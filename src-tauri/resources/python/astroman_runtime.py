"""AstroMan Python 运行时辅助模块（仅标准库）。

脚本由 AstroMan 套件引擎以子进程方式启动，通过 ``ASTROMAN_CONTEXT`` /
``ASTROMAN_RESULT`` 两个环境变量与 Rust 侧交换数据。也可以直接:

    import astroman_runtime as ar

    ctx = ar.load_context()
    ar.assert_eq(ctx["response"]["status"], 200, "状态码为 200")
    ar.emit()

约定：
* 断言失败不会抛异常，只写入结果，避免脚本提前退出丢失后续断言；
* ``emit()`` 可以多次调用，结果会累加后原子写回 ``result.json``；
* 附件路径必须位于 ``ASTROMAN_WORKDIR`` 内，越界会被 Rust 侧拒绝。
"""

from __future__ import annotations

import json
import os
import re
import sqlite3
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from http.cookiejar import CookieJar as _CookieJar
from typing import Any, Dict, Iterable, List, Optional, Tuple, Union

PROTOCOL_VERSION = "1.0"

_context: Optional[Dict[str, Any]] = None
_result: Dict[str, Any] = {
    "ok": True,
    "variables": {},
    "assertions": [],
    "logs": [],
    "attachments": [],
}
_opener_cache: Dict[bool, urllib.request.OpenerDirector] = {}


# ---------------------------------------------------------------------------
# 上下文与结果
# ---------------------------------------------------------------------------

def load_context(force: bool = False) -> Dict[str, Any]:
    """读取并缓存 context.json。"""
    global _context
    if _context is not None and not force:
        return _context
    path = os.environ.get("ASTROMAN_CONTEXT")
    if not path:
        raise RuntimeError(
            "缺少 ASTROMAN_CONTEXT 环境变量，脚本需由 AstroMan 套件引擎启动"
        )
    with open(path, "r", encoding="utf-8") as handle:
        context = json.load(handle)
    if not isinstance(context, dict):
        raise RuntimeError("context.json 内容必须是 JSON 对象")
    version = str(context.get("protocolVersion") or "")
    if not version.startswith("1."):
        raise RuntimeError(
            "不支持的协议版本：%s（本运行时要求 1.x）" % (version or "空")
        )
    _context = context
    return context


def work_dir() -> str:
    """返回本次用例的独立工作目录。"""
    return str(load_context()["paths"]["workDir"])


def _result_path() -> str:
    path = os.environ.get("ASTROMAN_RESULT")
    if not path:
        path = (load_context().get("paths") or {}).get("resultFile")
    if not path:
        raise RuntimeError("缺少 ASTROMAN_RESULT 环境变量")
    return str(path)


def _write_result() -> None:
    path = _result_path()
    directory = os.path.dirname(path)
    if directory:
        os.makedirs(directory, exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as handle:
        json.dump(_result, handle, ensure_ascii=False, indent=2)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(tmp, path)


def _normalize_logs(items: Optional[Iterable[Any]]) -> List[Dict[str, str]]:
    if not items:
        return []
    if isinstance(items, (str, bytes)):
        items = [items]
    out: List[Dict[str, str]] = []
    for item in items:
        if isinstance(item, dict):
            level = str(item.get("level") or "info").lower()
            message = str(item.get("message") or "")
        else:
            level, message = "info", str(item)
        if level == "warning":
            level = "warn"
        if level not in ("info", "warn", "error"):
            level = "info"
        out.append({"level": level, "message": message})
    return out


def _normalize_assertions(items: Optional[Iterable[Any]]) -> List[Dict[str, Any]]:
    if not items:
        return []
    out: List[Dict[str, Any]] = []
    for item in items:
        if not isinstance(item, dict):
            out.append({"name": "断言", "passed": bool(item), "message": ""})
            continue
        passed = bool(item.get("passed"))
        out.append(
            {
                "name": str(item.get("name") or "断言"),
                "passed": passed,
                "message": "" if passed else str(item.get("message") or ""),
            }
        )
    return out


def _normalize_attachments(items: Optional[Iterable[Any]]) -> List[Dict[str, str]]:
    if not items:
        return []
    out: List[Dict[str, str]] = []
    for item in items:
        if isinstance(item, dict):
            entry = {
                "name": str(item.get("name") or os.path.basename(str(item.get("path") or ""))),
                "path": str(item.get("path") or ""),
            }
            if item.get("mime"):
                entry["mime"] = str(item["mime"])
            out.append(entry)
        else:
            text = str(item)
            out.append({"name": os.path.basename(text) or "attachment", "path": text})
    return out


def log(message: Any, level: str = "info") -> None:
    """写一条日志（调用 emit 时统一落盘）。"""
    for entry in _normalize_logs([{"level": level, "message": message}]):
        _result["logs"].append(entry)


def emit(
    ok: bool = True,
    variables: Optional[Dict[str, Any]] = None,
    request: Optional[Dict[str, Any]] = None,
    assertions: Optional[Iterable[Any]] = None,
    logs: Optional[Iterable[Any]] = None,
    attachments: Optional[Iterable[Any]] = None,
    skip: Optional[bool] = None,
    fail_fast: Optional[bool] = None,
) -> Dict[str, Any]:
    """合并结果并原子写入 result.json（可多次调用）。"""
    _result["ok"] = bool(ok)
    if variables:
        if not isinstance(variables, dict):
            raise TypeError("variables 必须是 dict")
        _result["variables"].update(variables)
    if request is not None:
        if not isinstance(request, dict):
            raise TypeError("request 必须是 dict")
        _result["request"] = request
    _result["assertions"].extend(_normalize_assertions(assertions))
    _result["logs"].extend(_normalize_logs(logs))
    _result["attachments"].extend(_normalize_attachments(attachments))
    if skip is not None:
        _result["skip"] = bool(skip)
    if fail_fast is not None:
        _result["failFast"] = bool(fail_fast)
    _write_result()
    return _result


# ---------------------------------------------------------------------------
# 断言助手：失败不抛异常
# ---------------------------------------------------------------------------

def _short(value: Any, limit: int = 200) -> str:
    text = value if isinstance(value, str) else repr(value)
    if len(text) > limit:
        return text[:limit] + "...(%d 字符)" % len(text)
    return text


def _add_assertion(name: Any, passed: bool, message: str = "") -> bool:
    label = str(name or "断言")
    _result["assertions"].append(
        {
            "name": label,
            "passed": bool(passed),
            "message": "" if passed else str(message or ""),
        }
    )
    return bool(passed)


def assert_true(value: Any, name: str = "条件为真", message: Optional[str] = None) -> bool:
    passed = bool(value)
    return _add_assertion(
        name, passed, message if message is not None else "实际为假值：%s" % _short(value)
    )


def assert_false(value: Any, name: str = "条件为假", message: Optional[str] = None) -> bool:
    return assert_true(not value, name, message)


def assert_eq(
    actual: Any,
    expected: Any,
    name: Optional[str] = None,
    message: Optional[str] = None,
) -> bool:
    passed = actual == expected
    if not passed and isinstance(actual, str) and isinstance(expected, str):
        passed = actual.strip() == expected.strip()
    label = name or ("%s 等于 %s" % (_short(actual, 80), _short(expected, 80)))
    return _add_assertion(
        label,
        passed,
        message if message is not None else "期望 %s，实际 %s" % (_short(expected), _short(actual)),
    )


def assert_ne(
    actual: Any,
    expected: Any,
    name: Optional[str] = None,
    message: Optional[str] = None,
) -> bool:
    passed = not (actual == expected)
    label = name or ("%s 不等于 %s" % (_short(actual, 80), _short(expected, 80)))
    return _add_assertion(
        label,
        passed,
        message if message is not None else "不应等于 %s" % _short(expected),
    )


def assert_contains(
    haystack: Any,
    needle: Any,
    name: Optional[str] = None,
    message: Optional[str] = None,
) -> bool:
    if isinstance(haystack, dict):
        passed = needle in haystack
    elif isinstance(haystack, (list, tuple, set)):
        passed = any(item == needle for item in haystack)
    elif isinstance(haystack, str):
        passed = str(needle) in haystack
    elif haystack is None:
        passed = False
    else:
        passed = str(needle) in str(haystack)
    label = name or ("包含 %s" % _short(needle, 80))
    return _add_assertion(
        label,
        passed,
        message
        if message is not None
        else "%s 中未找到 %s" % (_short(haystack), _short(needle, 80)),
    )


_TYPES = {
    "object": dict,
    "array": list,
    "string": str,
    "number": (int, float),
    "integer": int,
    "boolean": bool,
    "null": type(None),
}


def _schema_errors(value: Any, schema: Any, path: str, out: List[str]) -> None:
    if len(out) >= 5 or not isinstance(schema, dict):
        return
    expected = schema.get("type")
    if expected:
        options = expected if isinstance(expected, list) else [expected]
        matched = False
        for option in options:
            py_type = _TYPES.get(str(option))
            if py_type is None:
                continue
            if py_type is int and isinstance(value, bool):
                continue  # bool 是 int 子类，单独判断
            if isinstance(value, py_type):
                matched = True
                break
        if not matched:
            out.append("%s 期望类型 %s，实际 %s" % (path, options, type(value).__name__))
            return

    if "enum" in schema and value not in schema["enum"]:
        out.append("%s 不在枚举 %s 中" % (path, _short(schema["enum"], 80)))
    if "const" in schema and value != schema["const"]:
        out.append("%s 期望常量 %s" % (path, _short(schema["const"], 80)))

    if isinstance(value, dict):
        for key in schema.get("required") or []:
            if key not in value:
                out.append("%s 缺少必填字段 %s" % (path, key))
        properties = schema.get("properties") or {}
        for key, child in properties.items():
            if key in value:
                _schema_errors(value[key], child, "%s.%s" % (path, key), out)
        if schema.get("additionalProperties") is False:
            for key in value:
                if key not in properties:
                    out.append("%s 存在未声明字段 %s" % (path, key))
    if isinstance(value, list):
        items = schema.get("items")
        if isinstance(items, dict):
            for index, child in enumerate(value[:50]):
                _schema_errors(child, items, "%s[%d]" % (path, index), out)
        if "minItems" in schema and len(value) < schema["minItems"]:
            out.append("%s 元素数 %d 小于 minItems=%s" % (path, len(value), schema["minItems"]))
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            out.append("%s 元素数 %d 大于 maxItems=%s" % (path, len(value), schema["maxItems"]))
    if isinstance(value, str):
        if "minLength" in schema and len(value) < schema["minLength"]:
            out.append("%s 长度 %d 小于 minLength=%s" % (path, len(value), schema["minLength"]))
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            out.append("%s 长度 %d 大于 maxLength=%s" % (path, len(value), schema["maxLength"]))
        pattern = schema.get("pattern")
        if pattern and not re.search(str(pattern), value):
            out.append("%s 未匹配正则 %s" % (path, pattern))
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            out.append("%s 小于 minimum=%s" % (path, schema["minimum"]))
        if "maximum" in schema and value > schema["maximum"]:
            out.append("%s 大于 maximum=%s" % (path, schema["maximum"]))
        if "exclusiveMinimum" in schema and value <= schema["exclusiveMinimum"]:
            out.append("%s 不大于 exclusiveMinimum=%s" % (path, schema["exclusiveMinimum"]))
        if "exclusiveMaximum" in schema and value >= schema["exclusiveMaximum"]:
            out.append("%s 不小于 exclusiveMaximum=%s" % (path, schema["exclusiveMaximum"]))


def assert_json_schema(
    data: Any, schema: Dict[str, Any], name: str = "符合 JSON Schema"
) -> bool:
    """内置迷你 JSON Schema 校验（type/required/properties/items/enum/数值/长度/正则）。"""
    errors: List[str] = []
    _schema_errors(data, schema, "$", errors)
    return _add_assertion(name, not errors, "；".join(errors))


# ---------------------------------------------------------------------------
# HTTP / DB
# ---------------------------------------------------------------------------

class Response:
    """``http()`` 的返回值，字段与套件引擎的响应快照保持一致。"""

    def __init__(
        self,
        status: int,
        headers: Dict[str, str],
        body: str,
        url: str,
        duration_ms: int,
    ) -> None:
        self.status = status
        self.headers = headers
        self.body = body
        self.url = url
        self.duration_ms = duration_ms

    @property
    def ok(self) -> bool:
        return 200 <= self.status < 400

    @property
    def text(self) -> str:
        return self.body

    def json(self) -> Any:
        return json.loads(self.body) if self.body else None

    def header(self, name: str, default: Optional[str] = None) -> Optional[str]:
        for key, value in self.headers.items():
            if key.lower() == name.lower():
                return value
        return default

    def __repr__(self) -> str:
        return "<Response %s %s %dms>" % (self.status, _short(self.url, 80), self.duration_ms)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: D401
        return None


def _opener(verify: bool, follow_redirects: bool) -> urllib.request.OpenerDirector:
    if verify and follow_redirects and verify in _opener_cache:
        return _opener_cache[verify]
    jar = _CookieJar()
    handlers: List[Any] = [urllib.request.HTTPCookieProcessor(jar)]
    if not follow_redirects:
        handlers.append(_NoRedirect())
    if not verify:
        context = ssl._create_unverified_context()  # noqa: SLF001
        handlers.append(urllib.request.HTTPSHandler(context=context))
    opener = urllib.request.build_opener(*handlers)
    if verify and follow_redirects:
        _opener_cache[verify] = opener
    return opener


def http(method: str, url: str, **kwargs: Any) -> Response:
    """轻量 HTTP 客户端（基于标准库 urllib）。

    支持 ``headers`` / ``params`` / ``json`` / ``data`` / ``timeout`` /
    ``verify`` / ``allow_redirects``，4xx/5xx 不抛异常，返回 Response。
    """
    headers = dict(kwargs.pop("headers", None) or {})
    params = kwargs.pop("params", None)
    json_body = kwargs.pop("json", None)
    data = kwargs.pop("data", None)
    timeout = kwargs.pop("timeout", None)
    verify = bool(kwargs.pop("verify", True))
    follow = bool(kwargs.pop("allow_redirects", True))
    if kwargs:
        raise TypeError("http() 不支持的参数：%s" % ", ".join(sorted(kwargs)))

    if params:
        query = urllib.parse.urlencode(params, doseq=True)
        url = url + ("&" if urllib.parse.urlparse(url).query else "?") + query

    body: Optional[bytes] = None
    if json_body is not None:
        body = json.dumps(json_body, ensure_ascii=False).encode("utf-8")
        if not any(key.lower() == "content-type" for key in headers):
            headers["Content-Type"] = "application/json; charset=utf-8"
    elif isinstance(data, dict):
        body = urllib.parse.urlencode(data, doseq=True).encode("utf-8")
        if not any(key.lower() == "content-type" for key in headers):
            headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif isinstance(data, str):
        body = data.encode("utf-8")
    elif data is not None:
        body = bytes(data)

    if timeout is None:
        limits = load_context().get("limits") or {}
        timeout = max(1.0, float(limits.get("timeoutMs") or 30000) / 1000.0)
    else:
        timeout = float(timeout)

    request = urllib.request.Request(
        url, data=body, headers=headers, method=str(method or "GET").upper()
    )
    started = time.perf_counter()
    try:
        with _opener(verify, follow).open(request, timeout=timeout) as response:
            raw = response.read()
            status = int(getattr(response, "status", response.getcode()))
            response_headers = dict(response.headers.items())
            final_url = response.geturl()
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        status = int(exc.code)
        response_headers = dict(exc.headers.items()) if exc.headers else {}
        final_url = exc.geturl()
    duration_ms = int((time.perf_counter() - started) * 1000)

    charset = "utf-8"
    for key, value in response_headers.items():
        if key.lower() == "content-type" and "charset=" in value.lower():
            charset = value.lower().split("charset=", 1)[1].split(";")[0].strip() or "utf-8"
            break
    try:
        text = raw.decode(charset, errors="replace")
    except LookupError:
        text = raw.decode("utf-8", errors="replace")
    return Response(status, response_headers, text, final_url, duration_ms)


def db_query(dsn: str, sql: str, params: Iterable[Any] = ()) -> List[Dict[str, Any]]:
    """执行 SQLite 查询并返回字典列表；其他数据库请自装驱动直连。"""
    text = str(dsn).strip()
    if text in (":memory:", "sqlite:///:memory:"):
        path = ":memory:"
    elif text.startswith("sqlite:///"):
        path = text[len("sqlite:///"):]
    elif text.startswith("sqlite://"):
        path = text[len("sqlite://"):]
    elif "://" in text:
        raise RuntimeError(
            "db_query 内置仅支持 SQLite；其他数据库请安装驱动后自行连接：%s" % text
        )
    else:
        path = text
    if path != ":memory:" and not os.path.isabs(path):
        path = os.path.join(work_dir(), path)
    connection = sqlite3.connect(path)
    connection.row_factory = sqlite3.Row
    try:
        cursor = connection.execute(sql, tuple(params or ()))
        return [dict(row) for row in cursor.fetchall()]
    finally:
        connection.close()


# ---------------------------------------------------------------------------
# 附件与脱敏
# ---------------------------------------------------------------------------

def _inside_workdir(path: str) -> Tuple[str, str]:
    root = os.path.realpath(work_dir())
    target = os.path.realpath(path if os.path.isabs(path) else os.path.join(root, path))
    try:
        inside = os.path.commonpath([root, target]) == root
    except ValueError:  # Windows 跨盘符
        inside = False
    if not inside:
        raise ValueError("附件路径越界（必须位于工作目录内）：%s" % target)
    return target, os.path.relpath(target, root)


def attach(
    name: str, path_or_bytes: Union[str, bytes, bytearray], mime: str = ""
) -> str:
    """登记附件；bytes 会写入工作目录，路径必须在工作目录内。"""
    if isinstance(path_or_bytes, (bytes, bytearray)):
        directory = os.path.join(work_dir(), "attachments")
        os.makedirs(directory, exist_ok=True)
        safe_name = os.path.basename(str(name or "attachment.bin")) or "attachment.bin"
        target = os.path.join(directory, safe_name)
        with open(target, "wb") as handle:
            handle.write(bytes(path_or_bytes))
    else:
        target = str(path_or_bytes)
        if not os.path.isabs(target):
            target = os.path.join(work_dir(), target)
        target, _ = _inside_workdir(target)
    entry: Dict[str, str] = {"name": str(name or os.path.basename(target)), "path": target}
    if mime:
        entry["mime"] = str(mime)
    _result["attachments"].append(entry)
    return target


def mask(value: Any) -> str:
    """日志中的轻量脱敏：保留首尾各 2 个字符。"""
    text = "" if value is None else str(value)
    if len(text) <= 6:
        return "******"
    return text[:2] + "******" + text[-2:]
