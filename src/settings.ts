import { invoke } from "@tauri-apps/api/core";

type Mods = { shift: boolean; ctrl: boolean; alt: boolean };
type SettingsShape = {
  enabled: boolean;
  trigger_vk: number;
  trigger_mods: Mods;
  trigger_mode: string;
  background: string;
  theme: string;
  categories: Record<string, boolean>;
  autostart: boolean;
};

const CATEGORY_LABELS: Array<[string, string]> = [
  ["code", "Code & configs"],
  ["text", "Plain text"],
  ["markdown", "Markdown"],
  ["json", "JSON / JSONC / JSONL"],
  ["table", "CSV / TSV tables"],
  ["sqlite", "SQLite databases"],
  ["log", "Log files"],
  ["env", ".env files"],
  ["sql", "SQL files"],
  ["notebook", "Jupyter notebooks"],
  ["archive", "Archives (zip/tar)"],
  ["media", "Images & media"],
];

const VK_NAMES: Record<number, string> = {
  0x20: "Space",
  0x0d: "Enter",
  0x09: "Tab",
  0x21: "Page Up",
  0x22: "Page Down",
};

let current: SettingsShape | null = null;
let recording = false;

function modsLabel(m: Mods): string[] {
  const parts: string[] = [];
  if (m.ctrl) parts.push("Ctrl");
  if (m.alt) parts.push("Alt");
  if (m.shift) parts.push("Shift");
  return parts;
}

function keyLabel(vk: number, m: Mods): string {
  const named = VK_NAMES[vk] ?? (vk >= 0x30 && vk <= 0x5a ? String.fromCharCode(vk) : `VK ${vk}`);
  const parts = modsLabel(m);
  return [...parts, named].join("+");
}

async function save(): Promise<void> {
  if (!current) return;
  await invoke("settings_set", { newSettings: current });
}

function renderCategoryList(s: SettingsShape): void {
  const list = document.querySelector<HTMLDivElement>("#category-list")!;
  list.replaceChildren(
    ...CATEGORY_LABELS.map(([key, label]) => {
      const row = document.createElement("div");
      row.className = "card-row";
      const text = document.createElement("div");
      text.className = "card-text";
      text.innerHTML = `<div class="card-label">${label}</div>`;
      const sw = document.createElement("label");
      sw.className = "switch";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = s.categories[key] ?? true;
      input.addEventListener("change", () => {
        if (!current) return;
        current.categories[key] = input.checked;
        void save();
      });
      const slider = document.createElement("span");
      slider.className = "slider";
      sw.append(input, slider);
      row.append(text, sw);
      return row;
    }),
  );
}

function reflect(s: SettingsShape): void {
  (document.querySelector<HTMLInputElement>("#opt-enabled")!).checked = s.enabled;
  const mode = document.querySelector<HTMLDivElement>("#opt-mode")!;
  for (const b of mode.querySelectorAll("button")) {
    b.classList.toggle("active", b.dataset["value"] === s.trigger_mode);
  }
  const theme = document.querySelector<HTMLDivElement>("#opt-theme")!;
  for (const b of theme.querySelectorAll("button")) {
    b.classList.toggle("active", b.dataset["value"] === s.theme);
  }
  (document.querySelector<HTMLInputElement>("#opt-autostart")!).checked = s.autostart;
  const bgSelect = document.querySelector<HTMLSelectElement>("#opt-background")!;
  bgSelect.value = /^#[0-9a-fA-F]{6}$/.test(s.background) ? "custom" : s.background;
  (document.querySelector<HTMLDivElement>("#row-custom-color")!).hidden =
    bgSelect.value !== "custom";
  if (bgSelect.value === "custom") {
    (document.querySelector<HTMLInputElement>("#opt-custom-color")!).value = s.background;
  }
  (document.querySelector<HTMLDivElement>("#hotkey-desc")!).textContent =
    `Current: ${keyLabel(s.trigger_vk, s.trigger_mods)}`;
  renderCategoryList(s);
}

async function load(): Promise<void> {
  current = await invoke<SettingsShape>("settings_get");
  reflect(current);
}

document.querySelector<HTMLInputElement>("#opt-enabled")!.addEventListener("change", (e) => {
  if (!current) return;
  current.enabled = (e.target as HTMLInputElement).checked;
  void save();
});

document.querySelector<HTMLDivElement>("#opt-mode")!.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest("button");
  if (!b || !current) return;
  current.trigger_mode = b.dataset["value"] ?? "toggle";
  reflect(current);
  void save();
});

document.querySelector<HTMLDivElement>("#opt-theme")!.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest("button");
  if (!b || !current) return;
  current.theme = b.dataset["value"] ?? "system";
  reflect(current);
  void save();
});

document.querySelector<HTMLSelectElement>("#opt-background")!.addEventListener("change", (e) => {
  if (!current) return;
  const v = (e.target as HTMLSelectElement).value;
  current.background = v === "custom" ? (document.querySelector<HTMLInputElement>("#opt-custom-color")!).value : v;
  reflect(current);
  void save();
});

document.querySelector<HTMLInputElement>("#opt-custom-color")!.addEventListener("input", (e) => {
  if (!current) return;
  current.background = (e.target as HTMLInputElement).value;
  void save();
});

document.querySelector<HTMLInputElement>("#opt-autostart")!.addEventListener("change", (e) => {
  if (!current) return;
  current.autostart = (e.target as HTMLInputElement).checked;
  void save();
});

// Hotkey recorder: capture the next key press (with modifiers).
// ev.keyCode is the Windows virtual-key code in WebView2, so every key works.
document.querySelector<HTMLButtonElement>("#btn-hotkey")!.addEventListener("click", () => {
  if (recording) return;
  recording = true;
  const desc = document.querySelector<HTMLDivElement>("#hotkey-desc")!;
  desc.textContent = "Press any key now... (Esc cancels)";
  const handler = (ev: KeyboardEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
    window.removeEventListener("keydown", handler, true);
    recording = false;
    if (!current) {
      return;
    }
    // Bare modifier presses cannot be triggers on their own (Alt alone opens
    // menus); tell the user instead of silently ignoring.
    if (["Shift", "Control", "Alt", "Meta"].includes(ev.key)) {
      desc.textContent = "Modifiers alone do not trigger: hold it with another key (e.g. Alt+K).";
      return;
    }
    current.trigger_vk = ev.keyCode;
    current.trigger_mods = {
      ctrl: ev.ctrlKey,
      alt: ev.altKey,
      shift: ev.shiftKey,
    };
    void save();
    reflect(current);
    desc.textContent = `Saved: ${keyLabel(current.trigger_vk, current.trigger_mods)}`;
  };
  window.addEventListener("keydown", handler, true);
});

void load();
