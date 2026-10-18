# 构建 GlassDock

## 在 Windows 上（推荐）
1. 安装 Rust：https://rustup.rs（选默认的 MSVC 工具链，需要 VS Build Tools）
2. 在本目录运行：
       cargo build --release
3. 产物：target\release\glassdock.exe

注意：`.cargo/config.toml` 是我在 Linux 上交叉编译用的（目标 x86_64-pc-windows-gnu + mingw 链接器）。
在 Windows 上用 MSVC 构建时，删掉或重命名 `.cargo` 文件夹即可。

## 在 Linux 上交叉编译
    sudo apt install gcc-mingw-w64-x86-64
    rustup target add x86_64-pc-windows-gnu
    cargo build --release        # 读取 .cargo/config.toml

## 文件
- src/main.rs      主循环、配置热重载、右键菜单动作
- src/gfx.rs       D3D11 设备、DXGI 桌面复制抓屏、着色器编译、D2D/DirectWrite
- src/glass.hlsl   玻璃着色器（折射、色散、磨砂、描边、阴影、自适应压暗）
- src/widget.rs    每个组件一个 DirectComposition 窗口；音乐卡片绘制、拖动、按钮
- src/media.rs     系统媒体控制（SMTC）：曲目、封面、播放控制
- src/config.rs    glassdock.toml 结构与默认材质
- preview/preview.html  着色器的 WebGL 镜像，用来在浏览器里预览/调参（?blur=4&rim=0.8 等）

- src/settings.rs 原生可视化设置窗口，数值校验与保存应用
