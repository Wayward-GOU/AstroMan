/* 对照表数据：ASCII、HTTP 状态码、常见 MIME、端口 */

export interface AsciiRow {
  dec: number
  hex: string
  bin: string
  char: string
  note: string
}

const CONTROL_NOTES: Record<number, string> = {
  0: 'NUL 空字符',
  1: 'SOH 标题开始',
  2: 'STX 正文开始',
  3: 'ETX 正文结束',
  4: 'EOT 传输结束',
  5: 'ENQ 询问',
  6: 'ACK 确认',
  7: 'BEL 响铃',
  8: 'BS 退格',
  9: 'HT 水平制表',
  10: 'LF 换行',
  11: 'VT 垂直制表',
  12: 'FF 换页',
  13: 'CR 回车',
  14: 'SO 移出',
  15: 'SI 移入',
  16: 'DLE 数据链路转义',
  17: 'DC1 设备控制 1',
  18: 'DC2 设备控制 2',
  19: 'DC3 设备控制 3',
  20: 'DC4 设备控制 4',
  21: 'NAK 否定确认',
  22: 'SYN 同步空闲',
  23: 'ETB 块传输结束',
  24: 'CAN 取消',
  25: 'EM 介质结束',
  26: 'SUB 替换',
  27: 'ESC 转义',
  28: 'FS 文件分隔符',
  29: 'GS 组分隔符',
  30: 'RS 记录分隔符',
  31: 'US 单元分隔符',
  32: 'SP 空格',
  127: 'DEL 删除',
}

export const ASCII_ROWS: AsciiRow[] = Array.from({ length: 128 }, (_, dec) => {
  const char = dec === 32 ? '␠' : dec === 127 ? '␡' : dec < 32 ? '·' : String.fromCharCode(dec)
  return {
    dec,
    hex: dec.toString(16).toUpperCase().padStart(2, '0'),
    bin: dec.toString(2).padStart(8, '0'),
    char,
    note: CONTROL_NOTES[dec] ?? (dec >= 48 && dec <= 57 ? '数字 0-9' : dec >= 65 && dec <= 90 ? '大写字母' : dec >= 97 && dec <= 122 ? '小写字母' : '可打印字符'),
  }
})

export interface HttpStatusRow {
  code: number
  name: string
  desc: string
  level: string
}

export const HTTP_STATUS_ROWS: HttpStatusRow[] = [
  { code: 100, name: 'Continue', desc: '继续发送请求体', level: '1xx 信息' },
  { code: 101, name: 'Switching Protocols', desc: '切换协议（WebSocket 升级）', level: '1xx 信息' },
  { code: 200, name: 'OK', desc: '请求成功', level: '2xx 成功' },
  { code: 201, name: 'Created', desc: '资源创建成功', level: '2xx 成功' },
  { code: 202, name: 'Accepted', desc: '请求已接受，异步处理', level: '2xx 成功' },
  { code: 204, name: 'No Content', desc: '成功但无返回内容', level: '2xx 成功' },
  { code: 206, name: 'Partial Content', desc: '部分内容（断点续传）', level: '2xx 成功' },
  { code: 301, name: 'Moved Permanently', desc: '永久重定向', level: '3xx 重定向' },
  { code: 302, name: 'Found', desc: '临时重定向', level: '3xx 重定向' },
  { code: 303, name: 'See Other', desc: '使用 GET 访问新地址', level: '3xx 重定向' },
  { code: 304, name: 'Not Modified', desc: '资源未修改，使用缓存', level: '3xx 重定向' },
  { code: 307, name: 'Temporary Redirect', desc: '保持原方法的临时重定向', level: '3xx 重定向' },
  { code: 308, name: 'Permanent Redirect', desc: '保持原方法的永久重定向', level: '3xx 重定向' },
  { code: 400, name: 'Bad Request', desc: '请求参数或格式错误', level: '4xx 客户端错误' },
  { code: 401, name: 'Unauthorized', desc: '未认证或凭证失效', level: '4xx 客户端错误' },
  { code: 403, name: 'Forbidden', desc: '无访问权限', level: '4xx 客户端错误' },
  { code: 404, name: 'Not Found', desc: '资源不存在', level: '4xx 客户端错误' },
  { code: 405, name: 'Method Not Allowed', desc: '方法不被允许', level: '4xx 客户端错误' },
  { code: 406, name: 'Not Acceptable', desc: '无法满足 Accept 要求', level: '4xx 客户端错误' },
  { code: 408, name: 'Request Timeout', desc: '请求超时', level: '4xx 客户端错误' },
  { code: 409, name: 'Conflict', desc: '资源冲突', level: '4xx 客户端错误' },
  { code: 410, name: 'Gone', desc: '资源已被永久删除', level: '4xx 客户端错误' },
  { code: 413, name: 'Payload Too Large', desc: '请求体过大', level: '4xx 客户端错误' },
  { code: 415, name: 'Unsupported Media Type', desc: '不支持的媒体类型', level: '4xx 客户端错误' },
  { code: 422, name: 'Unprocessable Entity', desc: '语义校验失败', level: '4xx 客户端错误' },
  { code: 429, name: 'Too Many Requests', desc: '请求频率超限', level: '4xx 客户端错误' },
  { code: 500, name: 'Internal Server Error', desc: '服务端内部错误', level: '5xx 服务端错误' },
  { code: 501, name: 'Not Implemented', desc: '功能未实现', level: '5xx 服务端错误' },
  { code: 502, name: 'Bad Gateway', desc: '网关收到无效响应', level: '5xx 服务端错误' },
  { code: 503, name: 'Service Unavailable', desc: '服务暂不可用或过载', level: '5xx 服务端错误' },
  { code: 504, name: 'Gateway Timeout', desc: '网关等待上游超时', level: '5xx 服务端错误' },
  { code: 505, name: 'HTTP Version Not Supported', desc: '不支持的 HTTP 版本', level: '5xx 服务端错误' },
]

export interface MimeRow {
  type: string
  ext: string
  desc: string
}

export const MIME_ROWS: MimeRow[] = [
  { type: 'application/json', ext: 'json', desc: 'JSON 数据' },
  { type: 'application/xml', ext: 'xml', desc: 'XML 数据' },
  { type: 'application/x-www-form-urlencoded', ext: '-', desc: '表单（键值对）' },
  { type: 'multipart/form-data', ext: '-', desc: '多部分表单（含文件）' },
  { type: 'application/octet-stream', ext: 'bin', desc: '二进制流' },
  { type: 'application/pdf', ext: 'pdf', desc: 'PDF 文档' },
  { type: 'application/zip', ext: 'zip', desc: 'ZIP 压缩包' },
  { type: 'application/javascript', ext: 'js', desc: 'JavaScript 脚本' },
  { type: 'application/problem+json', ext: 'json', desc: 'RFC 7807 错误响应' },
  { type: 'text/plain', ext: 'txt', desc: '纯文本' },
  { type: 'text/html', ext: 'html', desc: 'HTML 页面' },
  { type: 'text/css', ext: 'css', desc: '样式表' },
  { type: 'text/csv', ext: 'csv', desc: '逗号分隔值' },
  { type: 'text/event-stream', ext: '-', desc: 'SSE 事件流' },
  { type: 'text/markdown', ext: 'md', desc: 'Markdown 文档' },
  { type: 'image/png', ext: 'png', desc: 'PNG 图片' },
  { type: 'image/jpeg', ext: 'jpg', desc: 'JPEG 图片' },
  { type: 'image/gif', ext: 'gif', desc: 'GIF 图片' },
  { type: 'image/webp', ext: 'webp', desc: 'WebP 图片' },
  { type: 'image/svg+xml', ext: 'svg', desc: 'SVG 矢量图' },
  { type: 'audio/mpeg', ext: 'mp3', desc: 'MP3 音频' },
  { type: 'video/mp4', ext: 'mp4', desc: 'MP4 视频' },
]

export interface HeaderRow {
  header: string
  desc: string
  example: string
}

export const HEADER_ROWS: HeaderRow[] = [
  { header: 'Accept', desc: '期望的响应媒体类型', example: 'application/json' },
  { header: 'Accept-Encoding', desc: '可接受的压缩编码', example: 'gzip, deflate, br' },
  { header: 'Authorization', desc: '认证凭证', example: 'Bearer <token>' },
  { header: 'Cache-Control', desc: '缓存策略', example: 'no-cache' },
  { header: 'Content-Type', desc: '请求体媒体类型', example: 'application/json' },
  { header: 'Cookie', desc: '携带的 Cookie', example: 'session=abc123' },
  { header: 'If-None-Match', desc: '条件请求（ETag）', example: '"33a64df551425fcc"' },
  { header: 'Origin', desc: '跨域来源', example: 'https://example.com' },
  { header: 'Referer', desc: '来源页面', example: 'https://example.com/page' },
  { header: 'User-Agent', desc: '客户端标识', example: 'FinkMeter/1.0' },
  { header: 'X-Request-Id', desc: '链路追踪 ID', example: '9f1c2e5b' },
  { header: 'X-Forwarded-For', desc: '真实客户端 IP 链', example: '203.0.113.7' },
]

export interface CommandRow {
  command: string
  desc: string
  example: string
}

export const GIT_ROWS: CommandRow[] = [
  { command: 'git init', desc: '初始化本地仓库', example: 'git init my-project' },
  { command: 'git clone', desc: '克隆远程仓库', example: 'git clone https://github.com/user/repo.git' },
  { command: 'git status', desc: '查看工作区与暂存区状态', example: 'git status -sb' },
  { command: 'git add', desc: '把变更加入暂存区', example: 'git add src/main.ts' },
  { command: 'git commit', desc: '提交暂存区的内容', example: 'git commit -m "feat: 新增登录接口"' },
  { command: 'git log', desc: '查看提交历史', example: 'git log --oneline --graph -20' },
  { command: 'git diff', desc: '查看尚未暂存的差异', example: 'git diff --stat' },
  { command: 'git branch', desc: '列出、创建或删除分支', example: 'git branch -vv' },
  { command: 'git switch', desc: '切换分支（推荐替代 checkout）', example: 'git switch -c feature/login' },
  { command: 'git merge', desc: '合并分支到当前分支', example: 'git merge feature/login' },
  { command: 'git rebase', desc: '变基，把当前分支接到目标分支之后', example: 'git rebase main' },
  { command: 'git fetch', desc: '拉取远端更新但不合并', example: 'git fetch origin --prune' },
  { command: 'git pull', desc: '拉取并合并远端更新', example: 'git pull --rebase origin main' },
  { command: 'git push', desc: '推送本地提交到远端', example: 'git push -u origin main' },
  { command: 'git reset', desc: '回退提交或撤销暂存', example: 'git reset --soft HEAD~1' },
  { command: 'git restore', desc: '撤销工作区修改或取消暂存', example: 'git restore --staged file.txt' },
  { command: 'git stash', desc: '暂存当前未提交修改', example: 'git stash push -m "临时保存"' },
  { command: 'git tag', desc: '打标签，常用于发布版本', example: 'git tag -a v1.0.0 -m "首个版本"' },
  { command: 'git cherry-pick', desc: '把指定提交摘到当前分支', example: 'git cherry-pick a1b2c3d' },
  { command: 'git remote', desc: '管理远端仓库地址', example: 'git remote add origin git@github.com:user/repo.git' },
  { command: 'git config', desc: '读写仓库或全局配置', example: 'git config --global user.name "张三"' },
  { command: 'git blame', desc: '逐行查看最后修改者与提交', example: 'git blame -L 10,30 src/app.ts' },
]

export const DOCKER_ROWS: CommandRow[] = [
  { command: 'docker ps', desc: '列出运行中的容器', example: 'docker ps -a' },
  {
    command: 'docker images',
    desc: '列出本地镜像',
    example: 'docker images --format "table {{.Repository}}\\t{{.Tag}}"',
  },
  { command: 'docker run', desc: '创建并启动容器', example: 'docker run -d -p 8080:80 --name web nginx' },
  { command: 'docker start', desc: '启动已停止的容器', example: 'docker start web' },
  { command: 'docker stop', desc: '停止运行中的容器', example: 'docker stop web' },
  { command: 'docker restart', desc: '重启容器', example: 'docker restart web' },
  { command: 'docker rm', desc: '删除容器', example: 'docker rm -f web' },
  { command: 'docker rmi', desc: '删除镜像', example: 'docker rmi nginx:latest' },
  { command: 'docker exec', desc: '在运行中的容器里执行命令', example: 'docker exec -it web /bin/sh' },
  { command: 'docker logs', desc: '查看容器日志', example: 'docker logs -f --tail 100 web' },
  { command: 'docker inspect', desc: '查看容器或镜像的完整配置', example: 'docker inspect web' },
  { command: 'docker build', desc: '根据 Dockerfile 构建镜像', example: 'docker build -t myapp:1.0 .' },
  { command: 'docker pull', desc: '拉取镜像', example: 'docker pull mysql:8.0' },
  { command: 'docker push', desc: '推送镜像到仓库', example: 'docker push registry.example.com/myapp:1.0' },
  { command: 'docker tag', desc: '给镜像打标签', example: 'docker tag myapp:1.0 registry.example.com/myapp:1.0' },
  { command: 'docker compose up', desc: '按 compose 文件启动全部服务', example: 'docker compose up -d' },
  { command: 'docker compose down', desc: '停止并移除 compose 服务', example: 'docker compose down -v' },
  { command: 'docker compose logs', desc: '查看 compose 服务日志', example: 'docker compose logs -f api' },
  { command: 'docker stats', desc: '实时查看容器资源占用', example: 'docker stats --no-stream' },
  { command: 'docker system prune', desc: '清理未使用的容器、网络与镜像', example: 'docker system prune -a --volumes' },
]

export const REGEX_ROWS: CommandRow[] = [
  { command: '\\d', desc: '数字，等价于 [0-9]', example: '2024 → 2 0 2 4' },
  { command: '\\w', desc: '字母、数字或下划线', example: 'a_1 → a_1' },
  { command: '\\s', desc: '空白字符（空格、制表符、换行）', example: 'a b → 空格' },
  { command: '^', desc: '匹配行首', example: '^abc 只匹配开头是 abc 的行' },
  { command: '$', desc: '匹配行尾', example: 'xyz$ 只匹配结尾是 xyz 的行' },
  { command: '.', desc: '任意字符（不含换行，除非启用 s 标志）', example: 'a.c → abc / a1c' },
  { command: '*', desc: '前面的元素重复 0 次或多次', example: 'ab* → a / ab / abbb' },
  { command: '+', desc: '前面的元素重复 1 次或多次', example: 'ab+ → ab / abbb' },
  { command: '?', desc: '前面的元素出现 0 次或 1 次', example: 'colou?r → color / colour' },
  { command: '{n,m}', desc: '重复 n 到 m 次', example: '\\d{2,4} → 12 / 1234' },
  { command: '[abc]', desc: '字符集合中的任意一个', example: '[aeiou] → 匹配元音' },
  { command: '[^abc]', desc: '不在集合中的任意字符', example: '[^0-9] → 匹配非数字' },
  { command: '(abc)', desc: '捕获分组', example: '(\\d{4})-(\\d{2}) 取年月' },
  { command: '(?:abc)', desc: '非捕获分组，不占用分组编号', example: '(?:ab)+' },
  { command: '(?<name>abc)', desc: '命名捕获分组', example: '(?<year>\\d{4})' },
  { command: 'a|b', desc: '或，匹配 a 或 b', example: 'cat|dog' },
  { command: '(?=abc)', desc: '正向先行断言，后面必须是 abc', example: '\\d+(?=px)' },
  { command: '(?!abc)', desc: '负向先行断言，后面不能是 abc', example: '\\d+(?!px)' },
  { command: '(?<=abc)', desc: '正向后行断言，前面必须是 abc', example: '(?<=¥)\\d+' },
  { command: '(?<!abc)', desc: '负向后行断言，前面不能是 abc', example: '(?<!¥)\\d+' },
  { command: '\\b', desc: '单词边界', example: '\\bcat\\b → 独立单词 cat' },
  { command: '\\B', desc: '非单词边界', example: '\\Bcat' },
  { command: '\\u4e00-\\u9fa5', desc: '中文汉字范围', example: '[\\u4e00-\\u9fa5]+' },
]

