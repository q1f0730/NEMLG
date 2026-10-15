GlassDock v0.1 —— Windows 液态玻璃桌面小组件（原型）

运行
  双击 glassdock.exe。屏幕上会出现两块玻璃：时钟、音乐卡片。
  要求 Windows 10 2004 及以上或 Windows 11，免安装，不依赖其他运行库。

操作
  左键拖动        移动组件（位置会自动记住）
  音乐卡片按钮    上一首 / 播放暂停 / 下一首
  右键            菜单：始终置顶、编辑配置、重新加载配置、退出

网易云音乐
  客户端里打开：设置 → 系统 → 开启SMTC
  自带的 SMTC 没有进度条。想显示进度，请安装 InfLink-rs 插件。
  Spotify、QQ音乐、浏览器等任何接入系统媒体控制的播放器也能用。
  若想绑定其他播放器，把配置里 widget.prefer 改成 spotify / qqmusic 等。

配置渲染器
  第一次运行会在 exe 旁边生成 glassdock.toml，用记事本改完保存后立即生效。
  [material] 下是全部材质参数：
    blur 磨砂、refraction 折射、edge 透镜带宽度、aberration 色散、
    rim 描边、tint 底色、shadow 阴影、adaptive 背景过亮时自动压暗。

出问题时
  请把 exe 旁边的 glassdock.log 发给我。

原理
  DXGI 桌面复制实时抓屏，并把自身窗口排除在抓屏之外，避免反馈循环。
  D3D11 HLSL 着色器负责折射、色散、磨砂和描边，D2D/DirectWrite 绘制内容。
  每个组件是一个 DirectComposition 窗口，带逐像素透明。
