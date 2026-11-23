import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./RecordingIndicator.css";

interface AudioLevel {
  level: number;
  peak: number;
}

const CENTER_BAR = 18;
const BAR_COUNT = CENTER_BAR * 2 + 1;

export default function RecordingIndicator() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [processing, setProcessing] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    document.documentElement.classList.add("recording-window");
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const incoming = new Float32Array(CENTER_BAR + 1);
    const shown = new Float32Array(incoming.length);
    let width = 0;
    let height = 0;
    let recordingInk: CanvasGradient;
    let processingInk: CanvasGradient;
    let isProcessing = false;
    let processingSince: number | undefined;
    let frame: number | undefined;

    const schedule = () => {
      if (frame === undefined && !document.hidden) {
        frame = requestAnimationFrame(draw);
      }
    };

    const resize = () => {
      const bounds = canvas.getBoundingClientRect();
      width = bounds.width;
      height = bounds.height;
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      recordingInk = ctx.createLinearGradient(0, 0, width, 0);
      recordingInk.addColorStop(0, "#fa73dd");
      recordingInk.addColorStop(0.48, "#ae92ff");
      recordingInk.addColorStop(1, "#69f0e7");
      processingInk = ctx.createLinearGradient(0, 0, width, 0);
      processingInk.addColorStop(0, "#9f82ff");
      processingInk.addColorStop(1, "#83caff");
      schedule();
    };

    const draw = (now: number) => {
      frame = undefined;
      const animate = !reducedMotion.matches;
      let hasMotion = false;
      for (let i = 0; i < shown.length; i++) {
        const difference = incoming[i] - shown[i];
        if (animate && Math.abs(difference) > 0.01) {
          shown[i] += difference * 0.32;
          hasMotion = true;
        } else {
          shown[i] = incoming[i];
        }
      }

      processingSince ??= now;
      const sweep = animate
        ? ((now - processingSince) / 950) % 1.4 - 0.2
        : 0.5;
      const step = width / BAR_COUNT;
      const middle = height / 2;
      ctx.clearRect(0, 0, width, height);
      ctx.lineCap = "round";
      ctx.strokeStyle = isProcessing ? processingInk : recordingInk;

      ctx.beginPath();
      for (let i = 0; i < BAR_COUNT; i++) {
        const position = i / (BAR_COUNT - 1);
        const distance = Math.abs(i - CENTER_BAR);
        // Keep the newest audio at the centre so the meter responds at once.
        const intensity = isProcessing
          ? Math.exp(-Math.pow((position - sweep) / 0.17, 2)) * 0.77
          : shown[distance] * (1 - (distance / CENTER_BAR) * 0.55);
        const x = (i + 0.5) * step;
        const half = 0.8 + intensity * Math.max(0, middle - 4);
        ctx.moveTo(x, middle - half);
        ctx.lineTo(x, middle + half);
      }
      ctx.lineWidth = Math.min(4.5, step * 1.06);
      ctx.globalAlpha = 0.21;
      ctx.stroke();
      ctx.lineWidth = Math.min(2.3, step * 0.62);
      ctx.globalAlpha = 1;
      ctx.stroke();

      if (hasMotion || (isProcessing && animate)) schedule();
    };

    const unlistenAudio = listen<AudioLevel>("audio-level", ({ payload }) => {
      if (isProcessing) return;
      const strength = Math.max(payload.level * 1.55, payload.peak * 0.85, 0);
      incoming.copyWithin(1, 0);
      incoming[0] = Number.isFinite(strength) ? Math.min(strength, 1) : 0;
      schedule();
    });
    const unlistenProcessing = listen<boolean>("recording-processing", ({ payload }) => {
      isProcessing = payload;
      processingSince = undefined;
      incoming.fill(0);
      shown.fill(0);
      setProcessing(payload);
      schedule();
    });

    resize();
    window.addEventListener("resize", resize);
    document.addEventListener("visibilitychange", schedule);
    reducedMotion.addEventListener("change", schedule);

    return () => {
      if (frame !== undefined) cancelAnimationFrame(frame);
      window.removeEventListener("resize", resize);
      document.removeEventListener("visibilitychange", schedule);
      reducedMotion.removeEventListener("change", schedule);
      void unlistenAudio.then((unlisten) => unlisten());
      void unlistenProcessing.then((unlisten) => unlisten());
      document.documentElement.classList.remove("recording-window");
    };
  }, []);

  return (
    <div className="recording-pill" data-tauri-drag-region>
      <div
        className={`recording-pill__badge${processing ? " recording-pill__badge--processing" : ""}`}
        aria-hidden="true"
      >
        {processing ? (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
            <path d="M4 10v4m4-7v10m4-13v16m4-13v10m4-7v4" />
          </svg>
        ) : (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
            <rect x="9" y="3" width="6" height="11" rx="3" fill="currentColor" stroke="none" />
            <path d="M6 11v1a6 6 0 0 0 12 0v-1M12 18v3m-3 0h6" />
          </svg>
        )}
      </div>
      <div className="recording-pill__text" role="status">
        <span className="recording-pill__status">{processing ? "Transcribing" : "Recording"}</span>
        {!processing && <span className="recording-pill__shortcut"><kbd>Ctrl</kbd> + <kbd>Space</kbd> to stop</span>}
      </div>
      <canvas ref={canvasRef} className="recording-pill__wave" aria-hidden="true" />
    </div>
  );
}
