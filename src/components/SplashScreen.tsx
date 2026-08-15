import { useEffect, useRef, useState } from "react";
import lottie from "lottie-web";
import {
  CheckCircle2,
  CircleAlert,
  LoaderCircle,
  Sparkles,
} from "lucide-react";
import loadingAnimation from "../assets/loading.json";
import type { BootStep, BootStepState } from "../types/splash";

type SplashScreenProps = {
  steps: BootStep[];
  progress: number;
  version: string;
  finishing: boolean;
};

function StepIcon({ state }: { state: BootStepState }) {
  if (state === "done") {
    return <CheckCircle2 className="splash-step-icon splash-step-ok" size={17} />;
  }
  if (state === "warn" || state === "error") {
    return (
      <CircleAlert
        className={
          state === "error"
            ? "splash-step-icon splash-step-error"
            : "splash-step-icon splash-step-warn"
        }
        size={17}
      />
    );
  }
  if (state === "running") {
    return <LoaderCircle className="splash-step-icon splash-step-spin" size={17} />;
  }
  return <span className="splash-step-dot" />;
}

export function SplashScreen({
  steps,
  progress,
  version,
  finishing,
}: SplashScreenProps) {
  const animationRef = useRef<HTMLDivElement>(null);
  const [animationFailed, setAnimationFailed] = useState(false);

  useEffect(() => {
    const container = animationRef.current;
    if (!container) return;
    const animation = lottie.loadAnimation({
      container,
      renderer: "svg",
      loop: true,
      autoplay: true,
      animationData: loadingAnimation,
      rendererSettings: { preserveAspectRatio: "xMidYMid meet" },
    });
    animation.addEventListener("data_failed", () => setAnimationFailed(true));
    return () => {
      animation.destroy();
    };
  }, []);

  return (
    <div
      className={
        finishing ? "splash-screen splash-screen-leaving" : "splash-screen"
      }
      data-tauri-drag-region
    >
      <div className="splash-glow splash-glow-a" />
      <div className="splash-glow splash-glow-b" />
      <div className="splash-particles" aria-hidden="true">
        {Array.from({ length: 14 }, (_, index) => (
          <span key={index} style={{ "--i": index } as React.CSSProperties} />
        ))}
      </div>

      <div className="splash-inner">
        <div className="splash-brand" data-tauri-drag-region>
          <span className="splash-brand-mark">
            <Sparkles size={19} />
          </span>
          <strong>SH启动器</strong>
        </div>

        <div className="splash-stage">
          <div className="splash-halo" aria-hidden="true" />
          <div
            className={
              animationFailed
                ? "splash-anim splash-anim-fallback"
                : "splash-anim"
            }
            ref={animationRef}
            aria-label="正在加载"
          >
            {animationFailed ? <span className="splash-fallback-spinner" /> : null}
          </div>
        </div>

        <p className="splash-tagline">正在准备你的游戏世界…</p>

        <div className="splash-progress">
          <div className="splash-progress-track">
            <div
              className="splash-progress-fill"
              style={{ width: `${Math.round(progress)}%` }}
            />
          </div>
          <div className="splash-progress-meta">
            <span>{Math.round(progress)}%</span>
            <span>启动检查</span>
          </div>
        </div>

        <ul className="splash-checks">
          {steps.map((step) => (
            <li key={step.key} data-state={step.state}>
              <StepIcon state={step.state} />
              <span className="splash-check-label">{step.label}</span>
              <span className="splash-check-detail">{step.detail || "…"}</span>
            </li>
          ))}
        </ul>

        <p className="splash-version">
          v{version} · 本地数据仅保存在此设备上
        </p>
      </div>
    </div>
  );
}
