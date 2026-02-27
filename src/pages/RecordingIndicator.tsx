import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

interface AudioLevel {
  level: number; // 0.0 to 1.0
  peak: number;  // 0.0 to 1.0
}

export default function RecordingIndicator() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [levels, setLevels] = useState<number[]>(new Array(30).fill(0));
  const [isProcessing, setIsProcessing] = useState(false);

  // This window is 200×60px — kill any overflow that would show scrollbars
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    html.style.overflow = 'hidden';
    html.style.background = 'transparent';
    body.style.overflow = 'hidden';
    body.style.margin = '0';
    body.style.padding = '0';
    body.style.minWidth = '0';
    body.style.minHeight = '0';
    body.style.width = '100vw';
    body.style.height = '100vh';
    body.style.background = 'transparent';
    return () => {
      html.style.overflow = '';
      html.style.background = '';
      body.style.overflow = '';
      body.style.margin = '';
      body.style.padding = '';
      body.style.minWidth = '';
      body.style.minHeight = '';
      body.style.width = '';
      body.style.height = '';
      body.style.background = '';
    };
  }, []);

  useEffect(() => {
    // Listen for audio level updates
    const unlisten = listen<AudioLevel>('audio-level', (event) => {
      setLevels(prev => {
        const newLevels = [...prev.slice(1), event.payload.level];
        return newLevels;
      });
    });

    // Listen for processing state
    const unlistenProcessing = listen<boolean>('recording-processing', (event) => {
      setIsProcessing(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenProcessing.then(fn => fn());
    };
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    const barWidth = width / levels.length;
    const gap = 2;

    // Clear canvas
    ctx.clearRect(0, 0, width, height);

    if (isProcessing) {
      // Show processing animation
      ctx.fillStyle = '#3b82f6';
      const time = Date.now() / 200;
      for (let i = 0; i < levels.length; i++) {
        const h = (Math.sin(time + i * 0.3) + 1) * 0.3 * height + 4;
        const x = i * barWidth + gap / 2;
        const y = (height - h) / 2;
        ctx.fillRect(x, y, barWidth - gap, h);
      }
    } else {
      // Show audio waveform
      ctx.fillStyle = '#ef4444';
      levels.forEach((level, i) => {
        const h = Math.max(4, level * height * 0.9);
        const x = i * barWidth + gap / 2;
        const y = (height - h) / 2;
        ctx.fillRect(x, y, barWidth - gap, h);
      });
    }
  }, [levels, isProcessing]);

  // Animation loop for processing state
  useEffect(() => {
    if (!isProcessing) return;

    const interval = setInterval(() => {
      setLevels(prev => [...prev]); // Force re-render
    }, 50);

    return () => clearInterval(interval);
  }, [isProcessing]);

  return (
    <div
      className="w-screen h-screen flex items-center justify-center rounded-lg"
      style={{
        background: 'rgba(0, 0, 0, 0.85)',
        backdropFilter: 'blur(10px)',
      }}
      data-tauri-drag-region
    >
      {/* data-tauri-drag-region on the canvas too so the whole pill drags */}
      <canvas
        ref={canvasRef}
        width={180}
        height={40}
        className="rounded"
        data-tauri-drag-region
      />
    </div>
  );
}
