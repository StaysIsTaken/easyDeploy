import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import { api } from "../lib/api";
import { useJobs } from "../lib/jobs";

/** Live terminal of a job. Users can also type directly into it. */
export function JobTerminal({ jobId, interactive }: { jobId: string; interactive: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const { subscribe, buffer, input } = useJobs();
  const interactiveRef = useRef(interactive);
  interactiveRef.current = interactive;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const term = new XTerm({
      convertEol: false,
      cursorBlink: true,
      fontFamily: '"JetBrains Mono", "SF Mono", Menlo, Consolas, monospace',
      fontSize: 12.5,
      lineHeight: 1.25,
      scrollback: 10000,
      theme: {
        background: "#0d0f14",
        foreground: "#d8dbe5",
        cursor: "#8b8bff",
        selectionBackground: "#3b3f5c",
        black: "#1b1e27",
        brightBlack: "#5c6275",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    const doFit = () => {
      try {
        fit.fit();
        api.jobResize(jobId, term.cols, term.rows).catch(() => {});
      } catch {
        /* element not visible yet */
      }
    };
    requestAnimationFrame(doFit);
    for (const chunk of buffer(jobId)) term.write(chunk);
    const unsub = subscribe(jobId, (c) => term.write(c));
    const dataSub = term.onData((d) => interactiveRef.current && input(jobId, d));
    const ro = new ResizeObserver(() => doFit());
    ro.observe(el);
    return () => {
      ro.disconnect();
      unsub();
      dataSub.dispose();
      term.dispose();
    };
  }, [jobId, subscribe, buffer, input]);

  return <div className="terminal" ref={ref} />;
}
