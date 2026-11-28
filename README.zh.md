# Suppa Browser

<a href="https://www.buymeacoffee.com/Un1que0ff"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-violet.png" alt="Buy me a coffee" height="50"></a>

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · [Español](README.es.md) · **中文**

**一款小巧、快速、注重隐私的 Windows（现在也支持 Linux）网页浏览器——单个 3 MB 的 `.exe`，内置广告拦截、Chrome 扩展、标签页分组，界面可完全自定义。**

使用 Rust 基于 Microsoft **WebView2**（Windows 10/11 自带的 Chromium 引擎）编写——网页显示效果与 Chrome/Edge 完全一致，而 Suppa 本身非常小巧。

![Suppa Browser - 深色主题](docs/screenshots/main-dark.png)

---

## 亮点

- **小巧**——一个约 **3 MB** 的 `Suppa.exe`。没有 DLL，没有后台服务。可以用小巧的可选安装程序，也可以直接运行 exe。
- **快速**——窗口和工具栏约 **0.2 秒**就绪；广告拦截 **25 毫秒**就绪。
- **节省 CPU 和内存**——后台标签页会自动休眠和卸载；空闲时约占**一个 CPU 核心的 0.1%**。
- **注重隐私**——无需账户，没有遥测和分析。Suppa 本身从不向任何地方发送关于你的任何信息（详见下文）。
- **内置广告和跟踪器拦截**——Brave 的广告拦截引擎，包含 EasyList、EasyPrivacy、uBlock 过滤器和快速修复、Cookie 横幅列表，以及你自己的规则。
- **跳过 YouTube 广告**——漏网的广告会被静音、快进到结尾，并替你点击**跳过**按钮（使用 YouTube 认可的真实点击）；信息流和视频旁边的广告位会被隐藏。
- **自动处理 Cookie 弹窗**——默认**只接受必要的 Cookie**（“全部拒绝”），也可选择“全部接受”（设置 → 隐私；可关闭）。
- **地址、银行卡和 PayPal**——保存多个地址、银行卡和 PayPal 账户（在设置中输入，或从提交的表单保存），一键填写；填写银行卡前需通过 Windows Hello，安全码从不保存。
- **强密码**——注册表单会获得一个类似 `A%$3G&8^ga12Kq` 的生成密码，Suppa 会填写并为该网站保存。
- **通过 Dropbox 同步和共享**（可选）——密码、地址和银行卡保存在一个用你的口令加密的文件中，通过 Dropbox 应用同步；拥有相同文件夹和口令的每台电脑或每个人都能获得它们。
- **播放器和每个标签页的音量**——下载按钮旁的播放器按钮显示正在播放的内容（来自 Spotify / Tidal 的专辑封面、来自 YouTube 的小视频预览），可播放 / 暂停、下一首，并为每个标签页提供独立于 Windows 音量的音量滑块。标签页上会显示扬声器图标，点击即可静音。
- **保存视频 / MP3**——右键单击视频（或 YouTube 页面）即可保存，或只把声音保存为 MP3。
- **保持标签页唤醒**——你选择的标签页在后台永不休眠。
- **Chrome 扩展**——在 Chrome 网上应用店通过 **Add to Suppa** 安装，扩展按钮和弹窗显示在地址栏旁。
- **标签页分组**——把一个标签页拖到另一个上并按住：它们会组成一个可折叠的彩色分组。
- **已保存标签栏**，支持文件夹和拖放，☆ 一键保存网站。
- **你的布局**——标签页、地址栏和已保存标签页可分别放在任意位置：顶部、底部或左/右侧一栏（例如地址栏在顶部，标签页和已保存标签页在左侧）。
- **你的语言**——英语、俄语、德语、西班牙语或中文（默认使用 Windows 语言；设置 → 外观 → 语言）。
- **你的外观**——更深 / 深色 / 浅色 / 更浅主题，覆盖整个界面的色调，任意强调色（或 Windows 强调色），任意界面颜色，圆角或超椭圆角，选择标签页哪些角为圆角，五种图标样式，界面缩放。
- **无痕标签页**、标签页排序、把标签页拖出成为新窗口、崩溃恢复、备份与恢复、导入书签。
- **密码管理器**——查看、搜索、复制和删除已保存的密码（会先要求 Windows Hello 或 Windows 密码）；从 Firefox、Chrome、Edge 或 Brave 导入，并导出为 `.csv` 文件。
- **历史记录、下载和快捷方式**——可搜索的历史记录页面；地址栏旁的下载按钮会在下载时以强调色填充；起始页以你的主题显示最常访问的网站及其图标。
- **与主题相配的网站图标**——在标签页、已保存标签页、历史记录和起始页中；白色图标在浅色主题中变深，黑色图标在深色主题中变浅。
- **自动更新**——每天检查一次 GitHub；通过安装程序安装后，一键即可更新并显示新功能。
- **可随意修改**——整个界面都是 exe 旁文件夹里的普通 HTML/CSS/JS；修改后按 **Update** 即可。

---

## 截图

| | |
|---|---|
| ![深色主题](docs/screenshots/main-dark.png) | ![浅色主题](docs/screenshots/main-light.png) |
| **深色主题**（蓝色强调色）——标签页分组“Reading”、已保存标签栏、地址栏旁的下载按钮 | **浅色主题**（靛蓝强调色）——网站图标适应主题 |
| ![菜单](docs/screenshots/menu.png) | ![侧边栏](docs/screenshots/side-tabs.png) |
| **≡ 菜单**（橙色强调色）——所有菜单都跟随浏览器主题 | **左侧栏**（青柠色强调色）——竖排标签页、地址栏和已保存标签页在同一栏 |
| ![已保存标签页文件夹](docs/screenshots/saved-folder.png) | ![设置](docs/screenshots/settings.png) |
| **已保存标签页**（粉色强调色）——文件夹以菜单形式打开；拖动排序 | **设置**（黄色强调色）——左侧分组：外观、圆角、布局、隐私、密码、备份、更新 |
| ![密码管理器](docs/screenshots/passwords.png) | ![左侧标签页，红色色调](docs/screenshots/layout-tint.png) |
| **密码管理器**（红色强调色，演示数据）——搜索、显示、复制、删除；从 Firefox、Chrome、Edge 或 Brave 导入，导出为 `.csv` | **你的布局和外观**（玫瑰色强调色）——标签页在左侧，地址栏在顶部，深色主题加 20% 红色色调 |
| ![历史记录](docs/screenshots/history.png) | ![起始页和下载](docs/screenshots/downloads-start.png) |
| **历史记录**（`Ctrl+H`，紫色强调色）——按天分组，带网站图标和搜索 | **起始页**（青绿色强调色）——最常访问的网站及其图标，以及正在填充下载按钮的下载 |

![自定义颜色](docs/screenshots/settings-colors.png)
*设置 → 自定义颜色（青色强调色）：更改界面的任意颜色（浅色和深色主题分别设置）。*

---

## 速度和内存

在 Windows 10（WebView2 157）、Suppa 0.4、打开一个网页、启动 60 秒后测得：

| | |
|---|---|
| 程序大小 | **3.0 MB**（`Suppa.exe`，包含全部内容） |
| 窗口 + 工具栏就绪 | 启动后 **~0.19 秒** |
| 广告拦截就绪 | 启动后 **25 毫秒**（缓存的过滤引擎，在 UI 线程上 1-2 毫秒） |
| 空闲时 CPU | 所有 Suppa 进程合计**每分钟 63 毫秒 CPU 时间**（≈ 一个核心的 **0.1%**） |
| `Suppa.exe` 本身的内存 | **13 MB** |
| 整个浏览器（所有引擎进程，1 个页面） | **~270 MB** 私有内存，8 个进程 |

它为何轻量：

- **休眠标签页**——5 分钟没看的标签页会被冻结（0% CPU）；30 分钟后会被卸载并释放内存。点击标签页会重新加载。正在播放声音的标签页和保持唤醒的标签页永不休眠。（时间可在设置中更改。）
- **延迟恢复**——上次会话的标签页只在你点击时才加载。
- **共享进程**——同一网站的标签页共用一个渲染进程；工具栏和菜单共用另一个。
- **后台不运行任何东西**——通过 Windows 变更通知而不是轮询来发现文件变化；最小化的窗口不进行任何绘制。
- **快速绘制**——页面由 4 个光栅线程在显卡上绘制；大文件下载使用多个连接。

大部分内存属于 Chromium 引擎本身（页面渲染、GPU 进程、网络）。Suppa 额外只占约 13 MB。

---

## 隐私

**Suppa 不收集任何数据。** Suppa 的代码中没有账户、遥测、分析、崩溃报告，也不会“回传”任何信息。所有内容都保存在你电脑上的 `%LOCALAPPDATA%\SuppaBrowser` 中。

Suppa 自身发起的连接只有这些：

| 何时 | 连接到 | 用途 |
|---|---|---|
| 首次启动，之后每 4 天一次 | easylist.to、ublockorigin.github.io、secure.fanboy.co.nz、raw.githubusercontent.com | 下载广告过滤列表 |
| 仅在你点击 **Add to Suppa** 时 | clients2.google.com | 从 Chrome 网上应用店下载该扩展 |
| 每天一次（可在 设置 → 更新和文件 中关闭），以及你点击**检查更新**时 | api.github.com、github.com | 读取最新的 Suppa 版本号及其发行说明；在你点击**立即更新**时下载更新 |
| 你在地址栏中输入的内容 | 你选择的搜索引擎（Google、DuckDuckGo、Bing、Brave） | 你的搜索 |
| 仅在第一次使用**保存视频 / MP3** 时（会先询问你），之后保存内容时最多每周一次检查 yt-dlp 更新 | github.com | 将 yt-dlp 和 FFmpeg 下载到 Suppa 的数据文件夹（`tools\`） |

**Dropbox 同步**（默认关闭）本身不发起任何连接：Suppa 把一个文件写入你的 Dropbox 文件夹，由 Dropbox 应用进行同步。该文件用你的口令加密（PBKDF2-SHA256 + AES-256-GCM），Dropbox 无法读取。

额外的隐私保护：拦截广告和跟踪器，关闭 `<a ping>` 跟踪请求，禁用 Edge 的购物/翻译/SmartScreen 附加功能，无痕标签页关闭后不保留任何内容。

*网页由 Microsoft 的 WebView2 运行时（Windows 的一部分）显示。与任何 Windows 组件一样，它遵循 Windows 自己的诊断数据设置（设置 → 隐私 → 诊断和反馈）——这是 Windows 与 Microsoft 之间的事，与 Suppa 无关。*

---

## 功能

### 标签页
- **新建 / 关闭 / 重新打开**：`Ctrl+T`（或 `Ctrl+N`）、`Ctrl+W` 或鼠标中键，`Ctrl+Shift+T` 在原位置重新打开最后关闭的标签页。
- **排序**：向左/右拖动标签页——其他标签页会让开。平滑的打开/关闭动画；只有在一行放满时标签页才会变窄。
- **标签页分组**：拖动一个标签页并**停在另一个标签页的中间**→ 它们组成一个分组（或加入那个标签页的分组）。每个分组有一个彩色标签：点击折叠/展开，右键可重命名、更改颜色、取消分组或关闭分组。分组中的标签页带有分组颜色。右键单击标签页 → *添加到新组 / 从组中移除 / 取消分组*。分组随会话一起保存。
- **新窗口**：把标签页拖出窗口后松开——它会在新窗口中打开（登录状态和扩展相同）。
- **无痕**：`Ctrl+Shift+N`——独立的私密配置文件；不保存任何内容。
- **崩溃恢复**：每次更改 2 秒后保存打开的标签页和分组，崩溃后总能恢复。

### 已保存标签页（书签）
- 地址栏末尾的 ☆ 保存当前网站（★ = 已保存，再次点击移除），或右键单击标签页 → *保存*。
- 拖动排序。把一项拖到另一项的中间 → 生成包含两者的文件夹。拖到文件夹上 → 移入其中。把链接从打开的文件夹拖到栏上即可移出。
- 右键可选择*在新标签页 / 无痕中打开、重命名、删除、新建文件夹*。

### 广告拦截和 Cookie 弹窗
- 网络拦截、外观隐藏和反广告脚本（Brave 的 `adblock-rust` 引擎）。
- ≡ → *广告拦截*：查看拦截了多少请求、允许某网站显示广告、更新列表。
- 你自己的规则：`scripts\filters\*.txt`（uBlock / AdBlock 语法）。
- 自动处理 Cookie 横幅（支持多种语言）：默认**仅必要** Cookie（“全部拒绝”/“仅必要”，若无此按钮则点横幅的 ×），或**全部接受**——设置 → 隐私。也可在那里关闭。
- YouTube：视频广告会被静音、快进到结尾，并用真实（受信任）的鼠标点击按下**跳过**按钮，你无需自己点击。

### 地址、付款和密码
- **设置 → 地址和付款**：添加任意数量的地址（家、公司……）、银行卡和 PayPal 账户，或让 Suppa 在你提交表单时提示保存。所有内容都为你的 Windows 账户加密（`vault.dat`）。
- 网页表单的地址、银行卡、PayPal 和密码字段中会出现一个小**钥匙按钮**。它打开 Suppa 自己的菜单——网页永远看不到你保存的内容——只填写你选择的项目。银行卡在通过 Windows Hello 后填写；安全码（CVC）从不保存。如果商店同时支持 PayPal，菜单中会有**改用 PayPal 支付**；在 PayPal 登录页会填写你的 PayPal 邮箱。
- **强密码**：在注册表单中，钥匙按钮会提供一个生成的密码（大小写字母、数字、符号）。Suppa 会把它填入两个密码框，为该网站保存，并在下次启动时放入浏览器自己的密码库。设置 → 密码 中也有生成器（`Ctrl+Shift+G` 复制一个）。
- **通过 Dropbox 同步和共享**（设置 → 密码，默认关闭）：`Dropbox\Apps\Suppa` 中的一个文件，用你的口令加密。使用相同口令的每台电脑——以及你共享该 Dropbox 文件夹的每个人——都会获得相同的密码、地址和银行卡；更改按条目合并，删除也会同步。也可以改用任何其他同步文件夹（OneDrive、U 盘）。

### 媒体
- 当标签页在播放时，下载按钮旁会出现**播放器按钮**（≡ → 外观和工具栏 → 显示播放器按钮）：专辑封面（Spotify、Tidal、SoundCloud、YouTube Music……）或视频的小型实时预览（YouTube 和大多数视频网站），播放 / 暂停、上一首 / 下一首、静音，以及**仅针对该标签页的音量滑块**——Windows 音量保持不变。点击标题可转到该标签页。`Ctrl+Shift+M` 打开播放器。
- 正在播放的标签页显示**扬声器**（点击 = 静音，`Ctrl+M`）；右键单击标签页 → **声音**，可静音或设为 100 / 75 / 50 / 25 / 10% 音量。
- **保存视频 / MP3**：右键单击视频或音频（或 YouTube、Vimeo、SoundCloud 等页面上的任意位置）、播放器弹窗，或 ≡ → 此页面。普通视频文件直接下载；流媒体网站和 MP3 使用免费工具 yt-dlp 和 FFmpeg，Suppa 会在询问后下载一次。文件保存到你的“下载”文件夹。（请只保存你有权保存的内容。）
- **保持唤醒**：右键单击标签页 → *保持唤醒*（或 ≡ → 此页面）：它在后台永不冻结或卸载（标签页上会显示一个小太阳）。

### 扩展
- 在 Chrome 网上应用店打开一个扩展并点击 **Add to Suppa**（Suppa 会自行下载并安装）。
- 扩展按钮显示在地址栏右侧：点击 = 在按钮下方打开扩展弹窗，右键 = 选项 / 隐藏 / 移除。
- ≡ → *扩展* 列出所有扩展（启用/停用、选项、在工具栏显示、移除），并可加载已解压的文件夹。

### 外观和布局（设置）
- **语言**：English、Русский、Deutsch、Español 或 中文——菜单、设置、工具栏和 Suppa 的页面。*自动*（默认）使用 Windows 显示语言，若 Suppa 没有该语言则使用英语。翻译位于 `scripts\lang.json`。
- **主题**：**系统**（与 Windows 一样的浅色或深色——默认）、更深（黑色）、深色、浅色、更浅（白色）；**色调**：覆盖整个界面的任意颜色，0-60%（例如深色加 20% 红色）。强调色：每个标签页不同颜色、8 种预设、任意颜色或 **Windows 强调色**（默认）；强调色上的文字：自动 / 黑色 / 白色。
- **界面缩放** 75–150%（网页保留各自的缩放），**圆角程度** 0–20 px，**圆角样式**为圆形或**超椭圆**（处处使用平滑的超椭圆角，包括菜单）。
- **标签页圆角**：选择标签页四个角中哪些为圆角（或自动）。
- **图标样式**：线条、粗体、锐利、填充或双色（工具栏、已保存标签页、窗口按钮）。
- **设置 → 自定义颜色**：界面的每种颜色，按主题分别设置。
- **布局**：每一行（标签页、地址栏、已保存标签页）都可放在**顶部、底部、左侧或右侧**，也可一次全部设置；左/右侧的行与竖排标签页共用一栏（拖动其边缘调整宽度）。同一边缘的行按所选**顺序**排列。顶部没有任何行时，窗口按钮会浮在页面右上角。
- 拖动行下方的细线可调整其高度；≡ → *锁定栏大小* 可防止误操作。
- 窗口没有 Windows 标题栏——标签页行*就是*标题栏（拖动可移动，双击可最大化）。

### 密码、备份和导入
- 保存并填写密码和表单数据（可关闭）。
- **密码管理器**：设置 → 密码 列出所有已保存的密码，支持搜索、显示、复制、打开和删除（删除在 Suppa 重启时生效；引擎运行时会锁定其密码库）。**导出**将它们全部写入其他浏览器可导入的 `.csv` 文件。显示、复制和导出前会先让 Windows 确认你的身份（**Windows Hello** PIN、指纹或面部，或你的 Windows 密码）；一次验证一分钟内有效。
- **备份**：一个 `.zip`，包含设置、栏大小、已保存和打开的标签页、脚本、扩展、已保存的密码、表单数据和网站登录信息。**恢复**在下次启动时应用。（密码由 Windows 为你的账户加密，因此只能在同一 Windows 账户上恢复。）
- **导入书签**：从 Chrome、Edge、Brave、Vivaldi、Opera 或 Firefox（自动查找）或导出的书签 `.html` 文件导入。
- **导入密码**：从 Firefox、Chrome、Edge 或 Brave 的密码导出文件（`.csv`）导入：设置 → 密码 → 导入，或 ≡ → 备份和导入 → 从文件导入密码。在 Brave 中：brave://password-manager/settings → Export passwords。在 Firefox 中：about:logins → ⋯ → 导出登录信息。密码会立即为你的 Windows 账户加密，并在重启时进入 Suppa 的密码库；之后 Suppa 会提示删除导出文件（其中的密码是明文）。

### 历史记录和下载
- **历史记录**（`Ctrl+H`，≡ → 历史记录）：在普通标签页中打开过的每个页面，按天分组，可搜索；可删除单个页面、最近一小时或全部。首次启动时会复制引擎已有的历史记录。无痕标签页从不记录。
- **起始页快捷方式**：Suppa 起始页显示你最近 90 天最常访问的 8 个网站及其标签页图标（没有图标的网站显示彩色字母）；× 可移除。起始页使用你的主题和强调色，并显示 Suppa 版本。已保存标签页和历史记录页面显示相同的网站图标，因此导入的书签在你访问网站后就会获得图标。
- **下载**：不再有 Edge 的下载框。下载开始时会弹出显示进度的窗口，地址栏旁的下载按钮会随下载进度按顺时针方向以强调色填充（可在 ≡ → 显示下载按钮 中关闭）。点击 = 打开 / 暂停 / 继续，右键 = 在文件夹中显示、取消、重新下载、移除。**下载页面**（`Ctrl+J`）列出所有下载，支持搜索、打开、在文件夹中显示、暂停 / 继续和取消。

### 更新
- Suppa 每天向 GitHub 查询一次最新版本（只读取版本号；可在 设置 → 更新和文件 中关闭）。≡ → **检查更新** 会立即查询。
- 通过安装程序安装时：**立即更新** 会下载新的 `SuppaSetup.exe` 并静默运行；Suppa 关闭、更新后带着你的标签页重新打开。便携版则会打开下载页面。
- 更新后，**新功能** 会显示该版本在 GitHub 上的发行说明（也可在 设置 → 更新和文件 中查看）。

### 其他
- 所有地方（≡、标签页、已保存标签页、网页右键菜单）都使用主题化的菜单和对话框。右键单击网页 → **检查元素** 会在该元素上打开开发者工具。
- 将 Suppa 设为**默认浏览器**（在设置中或首次启动时的提示中）。
- 记住窗口大小和位置；全屏（`F11`）、打印、开发者工具、任务管理器。
- 你自己的用户脚本：`scripts\userscripts\*.js`（支持 `// @match` 规则）。

---

## 键盘快捷键

与 Chrome、Edge 和 Firefox 相同的按键。`F1`（≡ → 帮助和更新 → 键盘快捷键）可在 Suppa 中查看全部。

| 按键 | 操作 |
|---|---|
| `Ctrl+T` / `Ctrl+N` | 新标签页 |
| `Ctrl+Shift+N` | 新建无痕标签页 |
| `Ctrl+W`、`Ctrl+F4` / `Ctrl+Shift+W` | 关闭标签页 / 关闭窗口 |
| `Ctrl+Shift+T` | 重新打开关闭的标签页 |
| `Ctrl+Tab`、`Ctrl+PgDn` / `Ctrl+Shift+Tab`、`Ctrl+PgUp` | 下一个 / 上一个标签页 |
| `Ctrl+Shift+PgUp` / `Ctrl+Shift+PgDn` | 向左 / 向右移动标签页 |
| `Ctrl+1` … `Ctrl+9` | 转到第 1…8 个 / 最后一个标签页 |
| `Ctrl+M` | 静音 / 取消静音标签页 |
| `Ctrl+L`、`Alt+D`、`F6`、`Ctrl+E`、`Ctrl+K` | 地址栏（`Ctrl+Enter` 把 `name` 变成 `www.name.com`） |
| `F5`、`Ctrl+R` / `Ctrl+F5`、`Ctrl+Shift+R` | 重新加载 / 不使用缓存重新加载 |
| `Alt+Left` / `Alt+Right` / `Alt+Home` | 后退 / 前进 / 主页 |
| `Ctrl+D` / `Ctrl+Shift+B` | 将页面保存到已保存标签栏 / 显示或隐藏该栏 |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | 放大 / 缩小 / 重置缩放 |
| `Alt+S`、`Ctrl+,` | 设置 |
| `Alt+F`、`Alt+E`、`F10` | 打开 ≡ 菜单 |
| `F11` | 全屏 |
| `Ctrl+H` / `Ctrl+J` / `Shift+Esc` | 历史记录 / 下载 / 任务管理器 |
| `Ctrl+Shift+M` / `Ctrl+Shift+G` | 播放器 / 复制一个新的强密码 |
| `Ctrl+O` / `Ctrl+U` | 打开文件 / 页面源代码 |
| `Ctrl+Shift+Delete` | 清除浏览数据 |
| `Ctrl+P`、`Ctrl+F`、`F12`、`Ctrl+Shift+I` | 打印、查找、开发者工具 |
| `F1` | 快捷键列表 |

`Ctrl+D`、`Ctrl+E`、`Ctrl+K`、`Ctrl+M` 和 `Ctrl+U` 会先交给网页处理，与 Chrome 相同：自己使用这些按键的编辑器（`Ctrl+K` 插入链接、`Ctrl+U` 下划线）可以保留它们。

---

## 安装

**系统要求：** Windows 10 或 11（64 位），并安装 Microsoft Edge WebView2 Runtime（已更新的 Windows 10/11 中已包含）。

**安装程序（推荐）：** 从 [Releases](../../releases) 页面下载 **`SuppaSetup.exe`** 并运行。
- 只为你的 Windows 账户安装，无需管理员权限，安装到 `%LOCALAPPDATA%\Programs\Suppa`。
- 添加开始菜单和桌面快捷方式，并在 **Windows 设置 → 应用** 中添加条目，可在那里卸载。
- 如果缺少 WebView2 Runtime，会提示从 Microsoft 下载并安装。
- Suppa 会自动更新（设置 → 更新和文件），也可运行更新的 `SuppaSetup.exe`：你的标签页、设置、自定义过滤规则和用户脚本都会保留。
- 安装窗口跟随你的 Suppa 主题、强调色和圆角程度。
- 静默使用：`SuppaSetup.exe /S` 安装，`SuppaSetup.exe --uninstall /S` 卸载（保留你的数据）。

**便携版：** 从 [Releases](../../releases) 页面获取 `.zip`，解压到任意位置并运行 `Suppa.exe`。

**自行构建：** 双击 **`build.bat`**。它会安装缺少的组件（Rust、Microsoft C++ Build Tools、WebView2 运行时），然后构建 `dist\Suppa.exe`（旁边带有 `scripts` 文件夹）和 `dist\SuppaSetup.exe`。

**代码签名（可选）：** 如果有代码签名证书，`build.bat` 会为 `Suppa.exe` 和 `SuppaSetup.exe` 签名，使 Windows SmartScreen 信任它们。运行前在环境中设置以下之一（切勿写入项目中的文件）：`SUPPA_SIGN_THUMBPRINT`（Windows 证书存储中的证书）、`SUPPA_SIGN_PFX` + `SUPPA_SIGN_PASSWORD`（`.pfx` 文件）或 `SUPPA_SIGN_ARGS`（你自己的 `signtool` 参数，例如用于 Azure Trusted Signing）。需要 Windows SDK 中的 `signtool.exe`。不设置时，构建与以前一样不签名。

### Linux

Linux 版 Suppa 是同一款浏览器，基于 **GTK + WebKitGTK**（GNOME Web 的引擎）而不是 WebView2。它使用同一个 `scripts` 文件夹，因此工具栏、菜单、设置、主题、布局、已保存标签页、标签页分组、广告拦截、Cookie 弹窗处理、无痕标签页、崩溃恢复和 Update 按钮的工作方式都相同。

**系统要求：** 带有 WebKitGTK 4.1 的 64 位 Linux 桌面（大多数 GNOME 桌面已预装；否则使用 `sudo apt install libwebkit2gtk-4.1-0`、`sudo dnf install webkit2gtk4.1` 或 `sudo pacman -S webkit2gtk-4.1`）。

**下载：** 从 [Releases](../../releases) 页面获取 `Suppa-<version>-linux-x64.tar.gz`，解压后运行：

```sh
sh install.sh
```

它只为你的用户安装（无需 root）：程序在 `~/.local/share/suppa-browser`，`suppa` 命令在 `~/.local/bin`，并添加带图标的菜单项。`sh install.sh --uninstall` 可将其移除；你的设置和标签页保留在 `~/.local/share/SuppaBrowser`。

**自行构建：** `sh linux/build.sh` 会安装缺少的组件（Rust、WebKitGTK 开发文件），然后构建包含 `suppa`、`scripts/` 和 `install.sh` 的 `dist/linux/`。

Linux 版（暂时）没有：Chrome 扩展（WebKitGTK 不支持扩展）、保存密码和填写表单、历史记录和下载页面、更新、备份和书签导入、安装程序、冻结后台标签页（改为在设定时间后卸载），以及 0.12 的新功能（地址和付款、播放器、保存视频）。在 Wayland 桌面上，Suppa 通过 XWayland 运行，以便精确定位菜单。

---

## 无需重新编译即可自定义

`Suppa.exe` 旁边 `scripts\` 中的所有内容都可以编辑：

| 文件 | 控制内容 |
|---|---|
| `ui.html`、`ui.css`、`ui.js` | 工具栏：标签页、窗口按钮、地址栏、已保存标签页、颜色 |
| `menu.html` | 所有菜单、对话框和播放器弹窗 |
| `settings.html` | 设置标签页（左侧分组） |
| `lang.json`、`i18n.js` | 界面翻译（俄语、德语、西班牙语、中文） |
| `cookies.js` | 点击哪些 Cookie 按钮 |
| `youtube.js` | YouTube 广告跳过器（在 youtube.com 上广告拦截开启时运行） |
| `forms.js` | 表单中的钥匙按钮：地址、银行卡、PayPal、密码 |
| `media.js` | 播放器所需的数据和每个标签页的音量 |
| `webstore.js` | Chrome 网上应用店的“Add to Suppa”助手 |
| `newtab.html` | 可选的 Suppa 起始页（搜索和最常访问的网站） |
| `history.html`、`downloads.html` | 历史记录和下载页面 |
| `userscripts\*.js` | 你自己的网页脚本 |
| `filters\*.txt` | 你自己的广告拦截规则 |

文件更改后，**≡** 按钮会变成 **Update ≡**——点击 Update，Suppa 会带着你的更改重启（标签页会恢复）。如果你更改了项目文件夹中的 Rust 代码，Update 会先重新构建 `Suppa.exe`。

---

## 代码结构

| 文件 | 作用 |
|---|---|
| `src/main.rs` | 入口点和模块列表 |
| `src/app.rs` | 共享状态、标签页、分组、命令 ID |
| `src/window.rs` | 主窗口、消息循环、命令、窗口框架、更新/重启、崩溃恢复 |
| `src/ui.rs` | HTML 工具栏、工具栏与页面布局（每行可在顶部/底部/左侧/右侧）、缩放、主题颜色和色调、窗口图标 |
| `src/winbtn.rs` | 顶部没有任何行时浮在页面上的窗口按钮 |
| `src/tabs.rs` / `src/groups.rs` | 标签页（打开、关闭、排序、拆分）/ 标签页分组 |
| `src/webview.rs` | WebView2 设置、页面事件、请求拦截、页面右键菜单 |
| `src/adblock.rs` | 过滤列表、网络拦截、外观 CSS、脚本 |
| `src/extensions.rs` / `src/extpopup.rs` | Chrome 扩展 / 扩展弹窗 |
| `src/popup.rs` | 主题化的菜单和对话框 |
| `src/saved.rs` | 已保存标签栏、文件夹、星标按钮 |
| `src/settings.rs` / `src/settings_page.rs` | settings.ini、会话 / 设置标签页 |
| `src/backup.rs` / `src/import.rs` | 备份和恢复 / 书签导入 |
| `src/passwords.rs` / `src/auth.rs` | 密码管理器：列出、显示、复制、删除、导入和导出（`.csv`），使用引擎自己的加密存储 / Windows Hello 或账户密码验证 |
| `src/history.rs` / `src/downloads.rs` / `src/pages.rs` | 历史记录和起始页快捷方式 / 下载列表和弹窗 / 历史记录和下载页面 |
| `src/updater.rs` | 从 GitHub 发行版更新、新功能 |
| `src/sqlite.rs` | Windows 自带的 SQLite，用于引擎的密码和历史记录文件 |
| `src/sleeper.rs` | 休眠和卸载后台标签页（保持唤醒的除外） |
| `src/vault.rs` / `src/sync.rs` | 地址、银行卡、PayPal、生成的密码和表单填写（`scripts/forms.js`）/ 通过 Dropbox 同步和共享 |
| `src/i18n.rs` | 菜单和对话框的界面语言（`scripts/i18n.js` 翻译页面；两者都使用 `scripts/lang.json`） |
| `src/media.rs` / `src/savemedia.rs` | 播放器按钮和弹窗、每个标签页的音量（`scripts/media.js`）/ 保存视频和 MP3 |
| `src/scripts.rs` | scripts 文件夹、变更检测、重新构建 |
| `src/default_browser.rs`、`src/shortcuts.rs`、`src/menu.rs`、`src/util.rs` | 默认浏览器、快捷键、≡ 菜单、辅助函数 |
| `build.rs` | 嵌入清单，并根据 `assets/icon.svg` 绘制图标：深色 Windows 主题用透明背景上的白色图标，浅色主题用黑色图标（Suppa 随 Windows 切换） |
| `installer/` | `SuppaSetup.exe`：在 Suppa 风格的窗口中安装、更新和卸载 |
| `linux/` | Linux 版 Suppa（GTK + WebKitGTK）：相同的脚本、相同的设置代码（`src/settings.rs`），自己的窗口、标签页、菜单和广告拦截代码；`linux/build.sh`、`linux/install.sh` |
| `.github/workflows/release.yml` | 在 GitHub 上构建 `SuppaSetup.exe`、zip 和 tar.gz 并发布版本 |

---

## 已知限制

- Linux 版的功能比 Windows 版少（见 [Linux](#linux)）。
- 大多数 Chrome 扩展可以使用；少数需要 Chrome 专有 API 的扩展可能无法加载。
- 密码从其他浏览器导出的 `.csv` 文件导入，而不是直接读取其文件（Chrome 和 Edge 会锁定这些文件供自己使用）。

## 许可证

[MIT](LICENSE)
