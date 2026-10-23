# Presentation Shell Gate 原型

本项目只承载新 SHLNWindow 外壳及明确标注的占位内容，不绑定账号、下载或实例业务。原启动入口未切换。

构建：使用项目规定的 .NET SDK 构建本目录 ShellProbe.csproj，配置 Release。
运行：从项目根目录运行 python tools/run_shell_probe.py，或本机 .devdata/打开新外壳原型.cmd。窗口进程独立于启动它的编辑器。

直接运行 ShellProbe.dll 参数：
- 第一个参数 Solid / Native / InApp：材质对比。
- 第二个参数：可选的验收输出目录；有该参数会自动执行验收并退出。
- 输出目录名包含 review-2 时记录循环帧。记录是客户端离屏渲染，不能代替原生截图、物理 DPI 或实际显示帧率证明。
- SHLN_SHELL_GATE_EVIDENCE 可指向本机受保护证据目录，记录窗口几何/缩放；不记录表单内容或账号。
- measure_gpu.py PID OUTPUT_JSON 只读采样进程 GPU 引擎计数器，不修改系统设置。
