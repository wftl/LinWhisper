import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

interface AudioLevel {
  level: number; // 0.0 to 1.0
  peak: number;  // 0.0 to 1.0
}

export default function RecordingIndicator() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // Store levels in a ref so the draw function always has the latest value
  // without needing to go through React state on every audio event.
  const levelsRef = useRef<number[]>(new Array(30).fill(0));
  const isProcessingRef = useRef(false);
  const animFrameRef = useRef<number>(0);
  // Only used to trigger the RAF loop on/off — not for drawing.
  const [isProcessing, setIsProcessing] = useState(false);

  const drawCanvas = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const levels = levelsRef.current;
    const processing = isProcessingRef.current;
    const width = canvas.width;
    const height = canvas.height;
    const barWidth = width / levels.length;
    const gap = 2;

    ctx.clearRect(0, 0, width, height);

    if (processing) {
      ctx.fillStyle = '#3b82f6';
      const time = Date.now() / 200;
      for (let i = 0; i < levels.length; i++) {
        const h = (Math.sin(time + i * 0.3) + 1) * 0.3 * height + 4;
        const x = i * barWidth + gap / 2;
        const y = (height - h) / 2;
        ctx.fillRect(x, y, barWidth - gap, h);
      }
    } else {
      ctx.fillStyle = '#ef4444';
      levels.forEach((level, i) => {
        const h = Math.max(4, level * height * 0.9);
        const x = i * barWidth + gap / 2;
        const y = (height - h) / 2;
        ctx.fillRect(x, y, barWidth - gap, h);
      });
    }
  }, []);

  useEffect(() => {
    // Audio level updates: mutate the ref and redraw directly — no React state.
    const unlisten = listen<AudioLevel>('audio-level', (event) => {
      levelsRef.current = [...levelsRef.current.slice(1), event.payload.level];
      if (!isProcessingRef.current) {
        drawCanvas();
      }
    });

    const unlistenProcessing = listen<boolean>('recording-processing', (event) => {
      isProcessingRef.current = event.payload;
      setIsProcessing(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenProcessing.then(fn => fn());
    };
  }, [drawCanvas]);

  // Use requestAnimationFrame for the processing animation instead of setInterval
  // so we're synced to the display refresh rate and avoid forcing React re-renders.
  useEffect(() => {
    if (!isProcessing) {
      cancelAnimationFrame(animFrameRef.current);
      return;
    }

    const animate = () => {
      drawCanvas();
      animFrameRef.current = requestAnimationFrame(animate);
    };

    animFrameRef.current = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animFrameRef.current);
  }, [isProcessing, drawCanvas]);

  return (
    <div
      className="w-full h-full flex items-center justify-center rounded-lg"
      style={{
        background: 'rgba(0, 0, 0, 0.85)',
        backdropFilter: 'blur(10px)',
      }}
      data-tauri-drag-region
    >
      <canvas
        ref={canvasRef}
        width={180}
        height={40}
        className="rounded"
      />
    </div>
  );
}
