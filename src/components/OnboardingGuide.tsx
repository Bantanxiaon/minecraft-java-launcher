import { useState } from "react";

const STEPS = [
  {
    title: "欢迎使用 SH启动器",
    body: [
      "SH启动器可以帮你：下载并启动 Minecraft、导入整合包、管理模组、管理下载进度，并支持自动云更新。",
      "所有数据保存在 D 盘，不污染系统；安装包可随时在线升级。",
      "点击“下一步”开始 7 步引导，也可以随时跳过。",
    ],
  },
  {
    title: "第 1 步：创建游戏配置",
    body: [
      "回到主页，点击“+ 新建游戏配置”。",
      "填写配置名称，选择模组运行环境：原版 / Fabric / Forge / NeoForge / Quilt。",
      "选择 Minecraft 版本后点“创建游戏配置”。",
    ],
  },
  {
    title: "第 2 步：安装游戏与 Java",
    body: [
      "创建后点“检查并补齐游戏”，启动器会自动从官方源下载并校验游戏文件。",
      "有加载器时再点“安装模组环境”，会自动选择兼容版本。",
      "Java 不用管：启动器会按游戏版本自动安装 Java 8 / 17 / 21 并设为默认。",
    ],
  },
  {
    title: "第 3 步：开始游戏",
    body: [
      "配置状态变成“游戏文件已校验”后，回到主页点绿色“开始游戏”。",
      "启动前会自动做模组/前置检查；缺前置时可一键“自动补齐”，补齐失败会阻止启动。",
      "游戏运行中可点“强制结束游戏”终止无响应进程。",
    ],
  },
  {
    title: "第 4 步：整合包与模组",
    body: [
      "“整合包”页：把 .zip / .mrpack 拖进窗口或点按钮选择。",
      "每个整合包会自动创建一套独立实例，自动匹配游戏版本、加载器和 Java。",
      "“模组”页：拖入 .jar 或在线搜索（Modrinth + CurseForge，自动汉化），安装前自动校验兼容性。",
    ],
  },
  {
    title: "第 5 步：账户与外置登录",
    body: [
      "“账户”页可以创建本地离线档案，用来保存你的游戏身份。",
      "外置登录（如 LittleSkin）在“设置 → 常规 → 添加外置登录”填写 authlib 地址、用户名和密码。",
      "Microsoft 正版登录暂未开放；开放后会在“账户”页直接登录。",
    ],
  },
  {
    title: "第 6 步：更多功能",
    body: [
      "下载：底部进度条显示全部任务总进度，点开可查看每个下载/安装目标。",
      "设置：每套实例内存下拉选择（4–16GB 或自定义）、深色/浅色主题、下载模式与存储管理。",
      "侧栏“更新日志”实时显示版本内容；侧栏“使用教程”可随时查看完整功能说明。",
    ],
  },
];

export function OnboardingGuide({ onClose }: { onClose: () => void }) {
  const [step, setStep] = useState(0);
  const current = STEPS[step];
  const isLast = step === STEPS.length - 1;
  return (
    <div className="error-modal-backdrop" role="dialog" aria-modal="true" aria-label="开始游戏引导">
      <div className="changelog-modal onboarding-modal">
        <button
          className="btn btn-icon"
          aria-label="关闭"
          onClick={onClose}
        >
          ×
        </button>
        <div className="onboarding-head">
          <div>
            <h2>{current.title}</h2>
            <p className="changelog-subtitle">
              开始游戏引导 · 第 {step + 1} / {STEPS.length} 步
            </p>
          </div>
          <button
            className="onboarding-skip"
            type="button"
            onClick={onClose}
          >
            跳过引导
          </button>
        </div>
        <div className="onboarding-track">
          <div
            className="onboarding-track-fill"
            style={{ width: `${((step + 1) / STEPS.length) * 100}%` }}
          />
        </div>
        <div className="onboarding-body">
          {current.body.map((paragraph) => (
            <p key={paragraph}>{paragraph}</p>
          ))}
        </div>
        <div className="onboarding-actions">
          <button
            type="button"
            disabled={step === 0}
            onClick={() => setStep((value) => Math.max(0, value - 1))}
          >
            上一步
          </button>
          {isLast ? (
            <button className="primary" type="button" onClick={onClose}>
              开始使用
            </button>
          ) : (
            <button
              className="primary"
              type="button"
              onClick={() => setStep((value) => Math.min(STEPS.length - 1, value + 1))}
            >
              下一步
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
