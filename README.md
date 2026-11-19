# GlideQuill · 拂标

Windows x64 鼠标手势工具，使用 Rust 和 Win32。当前版本用于体验右键划动的轨迹与识别反馈，四个动作的实际执行留到下一阶段。

## 试用

1. 运行程序，打开「全局手势」。同时使用其他右键手势工具时，先暂停其中一个。
2. 在普通前台窗口按住右键，向一个方向划动。蓝色轨迹随指针出现，松键后在终点短暂显示对应动作。距离不足或形状不匹配的拖动显示「未识别」。
3. 轻点右键打开原有菜单。按住 Shift 可临时使用原生右键拖动。
4. 关闭设置窗口后可继续在托盘运行。双击托盘图标或再次启动可打开设置；托盘右键菜单可暂停、恢复或退出。

| 手势 | 识别结果 |
| --- | --- |
| ← 向左划 | 后退 |
| → 向右划 | 前进 |
| ↑ 向上划 | 页面顶部 |
| ↓ 向下划 | 页面底部 |

启动时处于暂停状态。按显示器 DPI 绘制，提供浅色和深色主题。[浅色实窗](docs/windows-light.png) · [深色实窗](docs/windows-dark.png)

## 运行与兼容

输入线程采样，窗口线程绘制透明、可穿透且不会激活的轨迹窗口。普通右击在松键时回放；全屏、提升权限的前台程序、交换主次键、按住修饰键和排除的程序放行。识别轨迹只在内存中短暂保存。

已有的 `%LOCALAPPDATA%\GlideQuill\rules.json` 保留在原处。本阶段只读取 `excluded` 排除列表。旧通用和浏览器绑定不会覆盖固定规则，也不会被重写。配置读取失败时暂停手势。

## 构建与验证

在 Windows x64 安装 Rust stable 和 MSVC C++ 构建工具，然后运行

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
.\target\release\glidequill.exe
```

在交互式 Windows 桌面执行以下检查

```powershell
cargo test --locked native_trail -- --ignored
pwsh -File .\scripts\smoke-test.ps1
```

桌面测试在独立前台窗口向输入处理函数提供坐标，检查松键前的实际轨迹像素、浮层穿透、识别提示和清除。冒烟测试使用临时配置，检查固定规则、主题、托盘、单实例和旧配置保留。

[产品需求与后续迭代](docs/Windows产品需求方案.md) · [早期 Figma 原型](https://www.figma.com/design/oQ0hbb7bNzsV0qiHrOI5ti/GlideQuill)
