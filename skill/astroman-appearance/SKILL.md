---
name: astroman-appearance
description: AstroMan 的外观规范：21 套配色主题（14 浅 / 7 深）的 35 个 --fm-* 变量约定、新增或调整主题的流程与对比度校验、航仔宇航员挂件的贴图与拖拽交互、1em 线框图标。改配色、加主题、换航仔形象、调挂件交互或截图核验时使用。触发词：主题、配色、换色、暗色、CSS 变量、--fm-、对比度、航仔、宇航员、挂件、拖拽、图标。
---

# 外观：主题与航仔

外观只有两件事：**颜色全部来自 `--fm-*` 变量**，**航仔是贴图不是矢量**。改这两块之前先读本页——不要在组件里写死 hex，也不要试图用 SVG 复刻渲染稿。

## 一、21 套主题

定义在 `src/stores/theme.ts`，`themes` 数组顺序就是设置页网格顺序。

| id | 名称 | 明暗 | 主色 | 定位 |
|---|---|---|---|---|
| `ocean` | 极光蓝 | 浅 | `#4d6bfe` | 默认 · 鲜明现代 |
| `graphite` | 石墨灰 | 浅 | `#466a8f` | 中性 · 专注数据 |
| `forest` | 森林绿 | 浅 | `#2e9e6b` | 自然 · 低饱和 |
| `midnight` | 暗夜 | 深 | `#82a6f0` | 深色 · 夜间调试 |
| `amber` | 琥珀橙 | 浅 | `#e0731c` | 暖调 · 品牌同色 |
| `teal` | 青碧 | 浅 | `#0d9488` | 清爽 · 冷绿调 |
| `rose` | 玫瑰 | 浅 | `#e0447a` | 柔和 · 暖粉调 |
| `clay` | 陶土红 | 浅 | `#b8564a` | 克制 · 暖而不刺眼 |
| `dusk` | 暮色 | 深 | `#96adff` | 深色 · 紫蓝夜空 |
| `violet` | 紫罗兰 | 浅 | `#7c5cf0` | 优雅 · 紫调 |
| `sky` | 天青 | 浅 | `#0ea5e9` | 明快 · 高饱和蓝 |
| `mint` | 薄荷 | 浅 | `#10b981` | 清凉 · 青绿 |
| `coral` | 珊瑚 | 浅 | `#f2714d` | 活力 · 暖橘红 |
| `cocoa` | 可可 | 浅 | `#9a6b4f` | 温润 · 大地棕 |
| `lime` | 青柠 | 浅 | `#65a30d` | 跳跃 · 黄绿 |
| `steel` | 钢蓝 | 浅 | `#4b7ea8` | 克制 · 冷灰蓝 |
| `indigo` | 靛夜 | 深 | `#8b9cff` | 深色 · 靛蓝 |
| `plum` | 梅夜 | 深 | `#cf8ad0` | 深色 · 紫红 |
| `pine` | 松夜 | 深 | `#6fc39a` | 深色 · 松林 |
| `slate` | 岩夜 | 深 | `#93b6cc` | 深色 · 岩板 |
| `aurora` | 极光青 | 深 | `#4fd1c5` | 深色 · 青碧流光 |

- 深色 7 套：`midnight` / `dusk` / `indigo` / `plum` / `pine` / `slate` / `aurora`；其余 14 套浅色
- 持久化键 `astroman.theme`，兼容旧的 `fbinhttp.theme` / `finkmeter.theme`（只读取一次，随后清掉）
- `applyTheme()` 把变量写到 `documentElement`，同时设 `data-theme="<id>"` 与 `style.colorScheme`；`initTheme()` 在 `main.ts` 里先跑，避免首屏闪白
- 判断明暗只用 `theme.dark`，不要在组件里比对主题名

## 二、35 个变量

`main.css` 的 `:root` 有 50 个 `--fm-*`，主题只覆盖其中 **35 个颜色 / 阴影**；HTTP 方法色 12 个（`--fm-method-*`）与圆角 3 个（`--fm-radius*`）**不随主题变化**。

| 组 | 变量 |
|---|---|
| 主色与语义 | `primary` `primary-dark` `primary-soft` `success` `success-soft` `warning` `warning-soft` `danger` `danger-soft` `purple` `purple-soft` |
| 背景与边框 | `bg` `surface` `surface-alt` `border` `border-strong` |
| 文字 | `text` `text-2` `muted` |
| 侧栏 | `sidebar` `sidebar-hover` `sidebar-active` `sidebar-text` `sidebar-icon` `sidebar-active-text` |
| 代码区 | `code-bg` `gutter-bg` `json-key` `json-string` `json-number` `json-bool` `json-punct` |
| 阴影 | `shadow-sm` `shadow` `shadow-lg` |

- `swatches` 固定 4 个值，顺序是 **`bg` / `surface-alt` / `primary` / `text`**，设置页缩略图按这个顺序取色
- 新增一套必须 35 个全给：漏掉的会回落到 `main.css` 的浅色默认值，在深色主题下通常直接不可读
- 变量名统一带 `--fm-` 前缀，写进 `ThemeDefinition.vars` 时**带前缀**（`'--fm-bg': '#fbf6f3'`）

## 三、新增一套主题

1. `ThemeId` 联合类型加 id（`theme.ts` 顶部）
2. 复制一套**同明暗**的 `ThemeDefinition` 改值，不要从零写
3. `id` / `name` / `desc` / `dark` / `swatches` / `vars` 六项齐全
4. 追加到 `themes` 数组——位置决定设置页顺序（例如「陶土红」插在「玫瑰」后面，就是第 8 个）
5. 跑对比度检查（下一节），再出图看整体
6. 同步文档：`CHANGELOG.md` 的「外观与航仔」段、`docs/features.md` 的主题清单、`docx` 里的现状表（主题数量在这三处都会出现）

```ts
const demo: ThemeDefinition = {
  id: 'demo',
  name: '示例色',
  desc: '一句话定位',
  dark: false,
  swatches: ['#fbf6f3', '#f8f0eb', '#b8564a', '#241715'],
  vars: {
    '--fm-bg': '#fbf6f3',
    // …共 35 项
  },
}
```

## 四、对比度硬指标

按 WCAG AA 起步，正文更严：

| 组合 | 下限 | 用途 |
|---|---|---|
| 白字 / `primary` | 4.5 | 主按钮 |
| `primary` 当文字 / `surface` | 4.5 | 链接、图标、选中态 |
| `text` / `bg` | 7 | 正文 |
| `muted` / `bg` | 4.5 | 说明文字、时间戳 |
| `sidebar-active-text` / `sidebar-active` | 4.5 | 侧栏选中 |
| `danger` / `danger-soft` | 4.5 | 危险按钮与提示 |

算对比度（node，复制即用）：

```js
function lum(h){const c=[1,3,5].map(i=>parseInt(h.substr(i,2),16)/255)
  .map(v=>v<=0.03928?v/12.92:Math.pow((v+0.055)/1.055,2.4));
  return 0.2126*c[0]+0.7152*c[1]+0.0722*c[2];}
function cr(a,b){const [x,y]=[lum(a),lum(b)].sort((m,n)=>n-m);
  return ((x+0.05)/(y+0.05)).toFixed(2);}
console.log(cr('#ffffff','#b8564a'), cr('#a5453a','#efdcd5'));
```

实测经验（陶土红）：`#b8564a` 配白字 **4.70**；侧栏选中文字最初 `#b04a3d` 压在 `#efdcd5` 上只有 **4.08**，调到 `#a5453a` 才到 **4.52**。

## 五、必须遵守的取舍

- **浅色底不要用纯白**：`#ffffff` / `#fdfaf9` 会让白色卡片和背景糊成一片。陶土红第一版底色 `#fdfaf9` 出图看不出层次，压到 `#fbf6f3` 才浮起来
- **危险色单独给**：`danger` 要比 `primary` 更纯（陶土红主色 `#b8564a`、危险色 `#c33a30`），否则删除 / 失败状态和主色分不清
- **深色主题不要混浅色卡片**：背景 `bg` 最暗，`surface` 亮一档，`surface-alt` 再亮一档
- **阴影别用纯黑**：浅色主题用带主色倾向的冷灰 / 暖灰，深色主题用更高透明度的黑
- **代码区四色要能互相区分**：`json-key` / `json-string` / `json-number` / `json-bool` 在同一 `code-bg` 上必须可辨，暗色下最容易糊

## 六、航仔挂件（`src/components/AstroBuddy.vue`）

形象是设计稿直接出图后抠的透明底贴图，**不是 SVG**。参考稿有真实高光、环境光和暗角，平面矢量画到「很像」就到头了，别回头再手绘一遍。

| 项 | 值 |
|---|---|
| 资源 | `src/assets/buddy.png`，288×483，透明底，243KB |
| 显示尺寸 | `W = 72` / `H = 121`（CSS 与脚本常量必须一致） |
| 位置 | `position: fixed` + `transform: translate3d(x, y, 0)`，默认贴右侧垂直居中 |
| 边界 | `EDGE = 10`，`clampPos()` 夹在窗口内；`resize` 时重新夹一次 |
| 拖拽 | Pointer Events + `setPointerCapture`，鼠标滑出角色范围仍跟手 |
| 点击判定 | 累计位移 ≤ **4px** 才当点击（`emit('open')`），否则拖拽并 `persist()` |
| 倾斜 | 水平步长 × 1.1 累加，衰减 0.55，夹在 ±13°，通过 CSS 变量 `--buddy-tilt` 传给 `.buddy-body` |
| 状态键 | `astroman.buddy.pos`（`{x,y}`）、`astroman.buddy.tip`（首次提示只出一次） |
| 无障碍 | `role="button"` + `tabindex="0"` + Enter / Space 打开；`prefers-reduced-motion` 下关闭全部动画 |

禁止项（都踩过）：

- 不要给 `<img>` 加白底 / 卡片背景，也不要用 `border-radius` 裁它
- 不要在 `.buddy`（外层）上写 `transform`——它已经用来做 `translate3d` 定位，会把位置覆盖掉；缩放放 `.buddy-body`
- 浮动动画和 hover 放大不要抢同一个元素，否则 `animation` 的暂停帧会压掉 `transform`
- 不要用 SVG `<filter>` 模拟投影，`drop-shadow` 两层（远 8px / 近 2px）就够
- 贴图放大后不要用 `image-rendering: pixelated`，源图是照片级渲染稿

## 七、1em 线框图标（`src/components/AstroBuddyIcon.vue`）

侧栏、空状态、AI 抽屉用它，**不要**把位图缩到 14px（会糊成一团）。

- `viewBox="0 0 24 24"`，`<svg>` 必须显式 `width="1em" height="1em"`，否则 flex 容器里高度会塌成 0
- 面罩做成实心：`<mask>` 里先铺白面罩 ellipse、再用黑色挖两只眼，然后拿它 mask 一整块 `currentColor`，这样单色也有「大面罩 + 双眼」的辨识度
- 躯干要比颈部明显宽（`rect` 宽 12.2 单位），否则整枚图标会被看成灯泡
- 只用 `currentColor` 与 `opacity`，不要写死颜色，让它跟随侧栏 / 按钮的语义色

## 八、出图核验（比读代码可靠）

```bash
npm run build
npx vite preview --port 4180 --strictPort     # 后台跑
```

```bash
# 截图输出到临时目录，别写项目目录（沙箱可能拒写）
"/c/Program Files/Google/Chrome/Application/chrome.exe" --headless=new --disable-gpu \
  --hide-scrollbars --force-device-scale-factor=2 --window-size=1280,800 \
  --virtual-time-budget=6000 --screenshot="C:\Users\<你>\AppData\Local\Temp\shot.png" \
  http://localhost:4180/
```

- 主题网格可以临时生一个静态 HTML（把 `swatches` 渲染成色卡），一次看 21 套的明暗与红/粉是否容易混淆
- 挂件这种小元素，截图后用 Pillow 裁区域再放大 2 倍看边缘有没有白边、灰边、抠图残渣
- 交互（拖拽、点击跳转）用 Chrome CDP：`Runtime.evaluate` 合成 PointerEvent，再读 `localStorage['astroman.buddy.pos']` 与 `location.hash` 验证；位移 < 4px 时不应跳转

## 九、常见问题

| 现象 | 原因 / 处理 |
|---|---|
| 卡片和背景糊在一起 | 底色太接近纯白；压深一档（如 `#fdfaf9` → `#fbf6f3`） |
| 侧栏选中文字发虚 | 选中文字与选中底色对比度不足 4.5；把文字压深而不是把底色加深 |
| 换了主题但部分区域没变 | 组件里写死了 hex 或用了 `rgba()` 常量；改成 `--fm-*` |
| 深色主题下 JSON 着色看不清 | `json-*` 四色与 `code-bg` 的明度差不够；暗色下把 key 提亮、string 降饱和 |
| 挂件拖一下变很小 / 位置乱跳 | 外层 `transform` 被覆盖；定位与倾斜要分开到两个元素 |
| 挂件边缘有白边 | 抠图容差太保守；四边泛洪的门限要包含渐变背景，alpha 再羽化 0.5px |
| 图标高度为 0 | `<svg>` 没写显式 `1em` 宽高，flex 里塌陷 |
| 悬停放大没反应 | hover 的 `transform` 被浮动动画覆盖；改到 `.buddy-body` 或换属性 |
