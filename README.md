# GlideQuill · 拂标

Windows x64 鼠标手势工具，Rust + Win32。0.1.0 是第一版可运行雏形。

## 实际 Windows 界面

左侧选择通用规则或浏览器范围，右侧直接编辑四个方向的动作并保存。底部练习区显示当前规则的匹配结果。[浅色窗口](docs/windows-light.png) · [深色窗口](docs/windows-dark.png)

## 本地试用

在 Windows x64 上使用 Rust stable（本次验证为 1.96.1）和 MSVC C++ 构建工具。

```powershell
cargo build --locked --release
.\target\release\glidequill.exe
```

1. 在「通用规则」里把向上划设为「复制」，在「浏览器覆盖」里把向上划设为「页面顶部」，分别保存。首次预设仅开启左右导航。
2. 在窗口底部按住右键画一条直线，查看识别结果。练习区可以试用尚未保存的当前选项，只显示匹配结果。
3. 在桌面上试用前，暂停已有的全局手势工具，然后点「启用全局手势」。程序每次启动均处于暂停状态。它只接管前台普通窗口中的手势；Shift + 右键可临时交回原应用，包括右键拖放。
4. 关闭主窗口收起到托盘。双击托盘图标或再次启动程序可打开设置，托盘右键可暂停、恢复或退出。

四个方向可分别设置前进、后退、复制、粘贴、跳到顶部或底部、最小化、最大化／还原或关闭窗口。其中前进后退发送 Alt + 左右键，顶部底部发送 Ctrl + Home／End，实际效果取决于目标程序。浏览器组按进程名匹配 Chrome、Edge、Firefox，可继承通用动作或显式停用。窗口动作只作用于起笔窗口，失去前台目标则放弃派发。

## 配置与边界

配置保存在当前用户的 `%LOCALAPPDATA%\GlideQuill\rules.json`。点击保存时原子替换；读取失败会提示并禁止保存覆盖。关闭程序后可以在 JSON 的 `excluded` 数组添加需要放行的进程文件名。浅／深色切换用于本次会话，部分系统控件使用 Windows 自身的颜色。

本版实现直线四向识别、规则持久化和实际输入派发。折线／自定义轨迹、任意快捷键录制、MouseInc 配置导入和完整 Figma 布局留在后续迭代。交换鼠标主次键的系统目前自动放行。手势启用时会延迟普通右击到松键时回放；依赖按下时机或右键自由拖动的应用可使用 Shift 放行或进程排除。

## 验证

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
pwsh -File .\scripts\smoke-test.ps1
```

冒烟测试需要已登录的 Windows 桌面；它启动 Release 程序，在独立临时配置目录验证试画、修改浏览器绑定、保存、重启回读、主题切换、托盘收起、单实例唤回与退出。测试通过窗口消息驱动练习区，不接管真实鼠标。高频鼠标、多屏 DPI、跨应用真实派发、右键透传和性能预算另需实机验收。

[完整需求与界面设计](docs/Windows产品需求方案.md) · [Figma 原型](https://www.figma.com/design/oQ0hbb7bNzsV0qiHrOI5ti/GlideQuill)
