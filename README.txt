GlassDock v0.1 —— Windows 液态玻璃桌面小组件（原型）

运行
  双击 glassdock.exe。屏幕上会出现音乐卡片。
  要求 Windows 10 2004 及以上或 Windows 11，免安装，不依赖其他运行库。

操作
  左键拖动        移动组件（位置会自动记住）
  音乐卡片按钮    上一首 / 播放暂停 / 下一首
  右键            菜单：始终置顶、设置、重新加载配置、退出

网易云音乐
  客户端里打开：设置 → 系统 → 开启SMTC
  自带的 SMTC 没有进度条。想显示进度，请安装 InfLink-rs 插件。
  Spotify、QQ音乐、浏览器等任何接入系统媒体控制的播放器也能用。
  若想绑定其他播放器，在右键 → 设置里的音乐来源填写 spotify / qqmusic 等。

设置渲染器
  右键 → 设置，通过窗口修改；点击保存并应用后立即生效。
  第一次运行会在 exe 旁边生成 glassdock.toml，自动记忆设置。
  [material] 下是全部材质参数：
    blur 磨砂、refraction 折射、edge 透镜带宽度、aberration 色散、
    rim 描边、tint 底色、shadow 阴影、adaptive 背景过亮时自动压暗。

出问题时
  请把 exe 旁边的 glassdock.log 发给我。

原理
  DXGI 桌面复制实时抓屏，并把自身窗口排除在抓屏之外，避免反馈循环。
  D3D11 HLSL 着色器负责折射、色散、磨砂和描边，D2D/DirectWrite 绘制内容。
  每个组件是一个 DirectComposition 窗口，带逐像素透明。

右键音乐卡片 → 设置…，打开实时玻璃设置面板。
拖动滑块实时预览，取消会撤销预览，点击“保存并应用”记住设置。
可用方向键微调、Tab 切换、Esc 取消；恢复默认可还原默认选项。
音乐来源支持快捷选择和自定义，无需编辑代码或配置。
也可用 glassdock.exe --settings 直接打开设置窗口。
