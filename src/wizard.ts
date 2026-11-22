import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Mods = { shift: boolean; ctrl: boolean; alt: boolean };

const state = {
  trigger_mode: "toggle",
  trigger_vk: 0x20,
  trigger_mods: { shift: false, ctrl: false, alt: false } as Mods,
};

const VK_NAMES: Record<number, string> = { 0x20: "Space", 0x0d: "Enter", 0x09: "Tab" };

function keyLabel(vk: number, m: Mods): string {
  const named = VK_NAMES[vk] ?? (vk >= 0x30 && vk <= 0x5a ? String.fromCharCode(vk) : `VK ${vk}`);
  const parts: string[] = [];
  if (m.ctrl) parts.push("Ctrl");
  if (m.alt) parts.push("Alt");
  if (m.shift) parts.push("Shift");
  return [...parts, named].join("+");
}

function showStep(step: "mode" | "hotkey"): void {
  (document.querySelector<HTMLElement>("#step-mode")!).hidden = step !== "mode";
  (document.querySelector<HTMLElement>("#step-hotkey")!).hidden = step !== "hotkey";
}

for (const card of document.querySelectorAll<HTMLButtonElement>(".mode-card")) {
  card.addEventListener("click", () => {
    for (const c of document.querySelectorAll<HTMLButtonElement>(".mode-card")) {
      c.classList.toggle("selected", c === card);
    }
    state.trigger_mode = card.dataset["value"] ?? "toggle";
    const next = document.querySelector<HTMLButtonElement>("#next-1")!;
    next.disabled = false;
  });
}

document.querySelector<HTMLButtonElement>("#next-1")!.addEventListener("click", () => {
  showStep("hotkey");
});

document.querySelector<HTMLButtonElement>("#back-2")!.addEventListener("click", () => {
  showStep("mode");
});

document.querySelector<HTMLButtonElement>("#wizard-change-key")!.addEventListener("click", () => {
  const label = document.querySelector<HTMLDivElement>("#wizard-hotkey-label")!;
  label.textContent = "Press any key now...";
  const box = label.closest(".hotkey-box");
  box?.classList.add("recording");
  const handler = (ev: KeyboardEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
    window.removeEventListener("keydown", handler, true);
    box?.classList.remove("recording");
    // Bare modifier presses cannot be triggers on their own (Alt alone opens
    // menus); tell the user instead of silently ignoring.
    if (["Shift", "Control", "Alt", "Meta"].includes(ev.key)) {
      label.textContent = "Modifiers alone do not trigger: hold it with another key (e.g. Alt+K).";
      return;
    }
    // ev.keyCode is the Windows virtual-key code in WebView2.
    const vk = ev.keyCode;
    state.trigger_vk = vk;
    state.trigger_mods = { ctrl: ev.ctrlKey, alt: ev.altKey, shift: ev.shiftKey };
    label.textContent = keyLabel(vk, state.trigger_mods);
    void invoke("settings_set", {
      newSettings: buildSettings(),
    })
      .then(() => {
        label.textContent = `${keyLabel(vk, state.trigger_mods)} (saved)`;
      })
      .catch((e) => {
        label.textContent = `Save failed: ${String(e).slice(0, 60)}`;
      });
  };
  window.addEventListener("keydown", handler, true);
});

function buildSettings() {
  return {
    enabled: true,
    trigger_vk: state.trigger_vk,
    trigger_mods: state.trigger_mods,
    trigger_mode: state.trigger_mode,
    background: "mica",
    theme: "system",
    categories: {
      code: true,
      text: true,
      markdown: true,
      json: true,
      table: true,
      sqlite: true,
      log: true,
      env: true,
      sql: true,
      notebook: true,
      archive: true,
      media: true,
    },
    autostart: false,
  };
}

document.querySelector<HTMLButtonElement>("#finish")!.addEventListener("click", () => {
  const finish = document.querySelector<HTMLButtonElement>("#finish")!;
  finish.disabled = true;
  finish.textContent = "Saving...";
  invoke("settings_set", { newSettings: buildSettings() })
    .then(() => getCurrentWindow().close())
    .catch((e) => {
      finish.disabled = false;
      finish.textContent = "Start using WinView";
      const err = document.querySelector<HTMLDivElement>("#wizard-error")!;
      err.hidden = false;
      err.textContent = `Could not save settings: ${String(e).slice(0, 120)}`;
    });
});
