/* UI coordinates and labels follow src/ui.rs; this is an operation mock. */
(() => {
  const $ = (id) => document.getElementById(id);
  const requestedVersion = new URL(location.href).searchParams.get("version");
  if (!requestedVersion || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(requestedVersion)) {
    throw new Error("Open the mock with ?version=<Cargo.toml version>.");
  }
  $("app-title").textContent = `win-cap v${requestedVersion} — 範囲録画`;
  const folder = "C:\\Users\\Demo\\Videos\\";
  const blank = () => ({ phase: "idle", region: null, fps: 30, cursor: true, filename: "capture.mp4", started: 0, changed: 0, completed: 0, notice: false });
  let state = blank();
  let mode = "interactive";
  const epoch = performance.now();
  const now = () => (performance.now() - epoch) / 1000;

  function transition(current, action, time) {
    const next = { ...current, changed: time };
    switch (action.type) {
      case "select":
        if (!["idle", "ready"].includes(current.phase)) throw new Error("Cannot select while recording or pending save.");
        next.phase = "selecting"; next.drag = null; break;
      case "drag-start":
        if (current.phase !== "selecting") throw new Error("Not selecting.");
        next.drag = { start: action.point, end: action.point }; break;
      case "drag-move":
        if (!current.drag) throw new Error("Selection drag has not started.");
        next.drag = { ...current.drag, end: action.point }; break;
      case "drag-end": {
        if (!current.drag) throw new Error("Selection drag has not started.");
        const [sx, sy] = current.drag.start;
        const [ex, ey] = action.point;
        const width = Math.floor(Math.abs(ex - sx) / 2) * 2;
        const height = Math.floor(Math.abs(ey - sy) / 2) * 2;
        if (width < 48 || height < 48) throw new Error("Select at least 48 × 48 pixels.");
        next.region = { x: Math.min(sx, ex), y: Math.min(sy, ey), width, height };
        next.drag = null; next.phase = "ready"; break;
      }
      case "cancel-selection": next.phase = current.region ? "ready" : "idle"; break;
      case "record":
        if (current.phase !== "ready" || !current.region) throw new Error("Select a region before recording.");
        next.phase = "recording"; next.started = time; next.completed = 0; break;
      case "stop":
        if (current.phase !== "recording") throw new Error("Not recording.");
        next.phase = "stopping"; next.completed = time - current.started; break;
      case "finished":
        if (current.phase !== "stopping") throw new Error("Not finalizing a recording.");
        next.phase = "save-dialog"; break;
      case "cancel-save":
        if (current.phase !== "save-dialog") throw new Error("Save dialog is not open.");
        next.phase = "pending"; break;
      case "retry-save":
        if (current.phase !== "pending") throw new Error("No recording pending save.");
        next.phase = "save-dialog"; break;
      case "save":
        if (current.phase !== "save-dialog" || !/^[^\\/:*?"<>|]+\.mp4$/i.test(current.filename)) throw new Error("Enter a new .mp4 filename.");
        next.phase = "saving"; break;
      case "saved":
        if (current.phase !== "saving") throw new Error("Not saving.");
        next.phase = "saved"; next.notice = true; next.savedPath = folder + current.filename; break;
      case "dismiss": next.notice = false; break;
      case "fps":
        if (!["idle", "ready", "saved"].includes(current.phase) || ![30, 60].includes(action.value)) throw new Error("Invalid FPS change.");
        next.fps = action.value; break;
      case "cursor": next.cursor = action.value; break;
      case "filename": next.filename = action.value; break;
      case "reset": return blank();
      default: throw new Error(`Unknown mock action: ${action.type}`);
    }
    return next;
  }

  const events = [
    [1.8, { type: "select" }],
    [2.4, { type: "drag-start", point: [36, 96] }],
    [3.8, { type: "drag-end", point: [676, 456] }],
    [6, { type: "record" }],
    [10.4, { type: "stop" }],
    [11.1, { type: "finished" }],
    [14.2, { type: "save" }],
    [14.8, { type: "saved" }],
    [19.1, { type: "reset" }],
  ];
  const cursorPoints = [
    [0, 925, 485], [1.1, 925, 485], [1.75, 543, 306],
    [2.35, 36, 96], [2.4, 36, 96], [3.8, 676, 456], [4.3, 741, 306],
    [5.8, 798, 306], [6.05, 798, 306], [6.3, 460, 180], [7.3, 215, 246],
    [8.4, 215, 294], [9.5, 215, 342], [10.4, 736, 480],
    [11.8, 740, 240], [13.6, 740, 240], [14.15, 621, 439],
    [14.55, 795, 475], [18.7, 795, 475], [19.1, 925, 485], [20, 925, 485],
  ];
  const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
  const ease = (x) => x * x * (3 - 2 * x);
  function cursorAt(time) {
    for (let i = 1; i < cursorPoints.length; i++) {
      if (time <= cursorPoints[i][0]) {
        const [start, x1, y1] = cursorPoints[i - 1];
        const [end, x2, y2] = cursorPoints[i];
        const fraction = ease(clamp((time - start) / (end - start), 0, 1));
        return [x1 + (x2 - x1) * fraction, y1 + (y2 - y1) * fraction];
      }
    }
    return cursorPoints.at(-1).slice(1);
  }
  function stepFor(current) {
    if (["idle", "selecting"].includes(current.phase)) return [1, "録画する範囲を選ぶ", "ドラッグした部分を録画します。"];
    if (current.phase === "ready") return [2, "録画を開始する", `${current.fps} fps・カーソル${current.cursor ? "あり" : "なし"}で開始。`];
    if (["recording", "stopping"].includes(current.phase)) return [3, "録画して、停止する", "停止: Ctrl + Shift + F10"];
    if (current.phase === "saved") return [4, "MP4として保存できました", "保存先は、録画を止めた後に選べます。"];
    return [4, "停止後に、保存先を選ぶ", "MP4の新しいファイル名を指定します。"];
  }
  function render(current, time) {
    const busy = ["recording", "stopping", "saving"].includes(current.phase);
    const pending = ["save-dialog", "pending"].includes(current.phase);
    const hiddenController = ["selecting", "recording", "stopping"].includes(current.phase);
    $("controller").hidden = hiddenController;
    $("select-button").disabled = busy || pending;
    $("record-button").disabled = !current.region || ["stopping", "saving"].includes(current.phase);
    $("record-button").textContent = busy ? "録画停止" : pending ? "録画を保存" : "録画開始";
    $("fps").disabled = busy || pending;
    $("fps").value = String(current.fps);
    $("include-cursor").disabled = busy || pending;
    $("include-cursor").checked = current.cursor;
    $("region-label").textContent = current.region ? `${current.region.width} × ${current.region.height} px　位置: ${current.region.x}, ${current.region.y}` : "範囲未選択";
    const messages = {
      idle: "範囲を選び、録画を開始してください。",
      selecting: "範囲を選び、録画を開始してください。",
      recording: "録画を準備しています…",
      ready: current.region && (current.region.width < 256 || current.region.height < 256) ? "MP4・音声なし（小範囲はCPU圧縮）" : "MP4・音声なし（GPU圧縮を優先）",
      stopping: "録画を停止しています…",
      "save-dialog": "録画完了・保存先を選択してください。",
      pending: "録画は残っています。「録画を保存」で再試行できます。",
      saving: "MP4を保存しています…",
      saved: "保存完了",
    };
    if (!Object.hasOwn(messages, current.phase)) throw new Error(`Unknown mock phase: ${current.phase}`);
    $("status").textContent = messages[current.phase];
    $("selection-overlay").hidden = current.phase !== "selecting";
    $("selection-rect").hidden = !current.drag;
    if (current.drag) {
      const [x1, y1] = current.drag.start;
      const [x2, y2] = current.drag.end;
      Object.assign($("selection-rect").style, { left: `${Math.min(x1, x2)}px`, top: `${Math.min(y1, y2)}px`, width: `${Math.abs(x2 - x1)}px`, height: `${Math.abs(y2 - y1)}px` });
    }
    $("save-dialog").hidden = current.phase !== "save-dialog";
    $("success-dialog").hidden = !current.notice;
    $("modal-background").hidden = current.phase !== "save-dialog" && !current.notice;
    $("filename").value = current.filename;
    $("saved-path").textContent = current.savedPath || "";
    const elapsed = ["recording", "stopping", "save-dialog", "pending", "saving", "saved"].includes(current.phase) ? (current.completed || time - current.started) : 0;
    const completedRows = Math.min(3, Math.floor(Math.max(0, elapsed) / 1.1));
    const checklist = ["共有したい内容を確認", "必要な操作を実行", "結果を確認して完了"];
    $("checklist").innerHTML = checklist.map((text, i) => `<div class="check-row ${i < completedRows ? "complete" : ""}"><span class="check-box">✓</span><span class="check-text">${text}</span></div>`).join("");
    const [step, title, detail] = stepFor(current);
    $("step-number").textContent = `0${step}`;
    $("step-title").textContent = title;
    $("step-detail").textContent = detail;
    $("step-progress").innerHTML = [1, 2, 3, 4].map((n) => `<i class="step-dot ${n <= step ? "active" : ""}"></i>`).join("");
    $("demo-cursor").hidden = mode !== "capture";
    $("click-ring").hidden = true;
    $("loop-fade").style.opacity = "0";
    if (mode === "capture") {
      const [x, y] = cursorAt(time);
      Object.assign($("demo-cursor").style, { left: `${x}px`, top: `${y}px` });
      $("demo-cursor").classList.toggle("crosshair", current.phase === "selecting");
      const click = [1.8, 2.4, 6, 14.2].find((at) => time >= at && time < at + 0.4);
      if (click !== undefined) {
        const progress = (time - click) / 0.4;
        const [cx, cy] = cursorAt(click);
        $("click-ring").hidden = false;
        Object.assign($("click-ring").style, { left: `${cx - 22}px`, top: `${cy - 22}px`, opacity: `${1 - progress}`, transform: `scale(${0.65 + progress * 0.55})` });
      }
      const fade = time > 18.7 && time < 19.1 ? (time - 18.7) / 0.4 : time >= 19.1 && time < 19.5 ? (19.5 - time) / 0.4 : 0;
      $("loop-fade").style.opacity = String(clamp(fade, 0, 1));
    }
    window.demoState = structuredClone(current);
  }
  function dispatch(action) {
    state = transition(state, action, now());
    render(state, now());
  }
  function toggle() {
    if (state.phase === "recording") dispatch({ type: "stop" });
    else if (state.phase === "pending") dispatch({ type: "retry-save" });
    else if (["ready", "saved"].includes(state.phase)) {
      if (state.phase === "saved") state = { ...state, phase: "ready" };
      dispatch({ type: "record" });
    } else if (state.phase === "idle") dispatch({ type: "select" });
  }
  $("select-button").addEventListener("click", () => {
    if (state.phase === "saved") state = { ...state, phase: "ready" };
    dispatch({ type: "select" });
  });
  $("record-button").addEventListener("click", toggle);
  $("fps").addEventListener("change", (event) => dispatch({ type: "fps", value: Number(event.target.value) }));
  $("include-cursor").addEventListener("change", (event) => dispatch({ type: "cursor", value: event.target.checked }));
  $("filename").addEventListener("input", (event) => dispatch({ type: "filename", value: event.target.value }));
  $("save-button").addEventListener("click", () => dispatch({ type: "save" }));
  $("cancel-save").addEventListener("click", () => dispatch({ type: "cancel-save" }));
  $("success-ok").addEventListener("click", () => dispatch({ type: "dismiss" }));
  function point(event) {
    const bounds = $("desktop").getBoundingClientRect();
    return [Math.round(clamp(event.clientX - bounds.left - 1, 0, 974)), Math.round(clamp(event.clientY - bounds.top - 1, 0, 510))];
  }
  $("selection-overlay").addEventListener("pointerdown", (event) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    dispatch({ type: "drag-start", point: point(event) });
  });
  $("selection-overlay").addEventListener("pointermove", (event) => {
    if (state.drag) dispatch({ type: "drag-move", point: point(event) });
  });
  $("selection-overlay").addEventListener("pointerup", (event) => {
    dispatch({ type: "drag-end", point: point(event) });
    event.currentTarget.releasePointerCapture(event.pointerId);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && state.phase === "selecting") dispatch({ type: "cancel-selection" });
    if (event.ctrlKey && event.shiftKey && event.key === "F9") { event.preventDefault(); toggle(); }
    if (event.ctrlKey && event.shiftKey && event.key === "F10" && state.phase === "recording") { event.preventDefault(); dispatch({ type: "stop" }); }
  });
  window.renderDemoFrame = (time) => {
    if (!Number.isFinite(time) || time < 0 || time > 20) throw new Error("Demo time must be within 0..20 seconds.");
    mode = "capture";
    let frame = blank();
    for (const [at, action] of events) if (time >= at) frame = transition(frame, action, at);
    if (time >= 2.4 && time < 3.8) frame = transition(frame, { type: "drag-move", point: cursorAt(time) }, time);
    render(frame, time);
    return window.demoState;
  };
  window.resetDemo = () => { mode = "interactive"; state = blank(); render(state, now()); };
  function tick() {
    if (mode === "interactive") {
      const time = now();
      if (state.phase === "stopping" && time - state.changed >= 0.7) state = transition(state, { type: "finished" }, time);
      if (state.phase === "saving" && time - state.changed >= 0.6) state = transition(state, { type: "saved" }, time);
      render(state, time);
    }
    requestAnimationFrame(tick);
  }
  render(state, now());
  requestAnimationFrame(tick);
})();
