import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "katex/dist/katex.min.css";

type Preview =
  | { kind: "text"; data: { content: string; truncated: boolean } }
  | { kind: "code"; data: { html: string; lang: string; truncated: boolean } }
  | {
      kind: "markdown";
      data: { html: string; mermaid: string[]; raw: string; truncated: boolean };
    }
  | { kind: "diff"; data: { html: string; truncated: boolean } }
  | {
      kind: "package_json";
      data: { summary: Record<string, unknown>; code_html: string; truncated: boolean };
    }
  | { kind: "json_tree"; data: { html: string; raw_html: string; truncated: boolean } }
  | { kind: "json_lines"; data: { lines: string[]; count: number } }
  | {
      kind: "table";
      data: {
        columns: string[];
        numeric: boolean[];
        total_rows: number;
        sample_rows: string[][];
        decimal_comma: boolean;
        truncated: boolean;
      };
    }
  | {
      kind: "sqlite";
      data: { tables: SqliteTable[]; wal_warning: boolean; error: string | null };
    }
  | {
      kind: "log";
      data: {
        html: string;
        first_line_no: number;
        total_lines: number;
        truncated: boolean;
        tail_only: boolean;
      };
    }
  | { kind: "env"; data: { entries: EnvEntry[] } }
  | {
      kind: "sql";
      data: { formatted_html: string; raw_html: string; fallback_reason: string | null };
    }
  | {
      kind: "notebook";
      data: { cells: NotebookCell[]; script: string; error: string | null };
    }
  | {
      kind: "archive";
      data: {
        kind: string;
        entries: Array<{ path: string; size: number; compressed: number | null }>;
        total_files: number;
        total_bytes: number;
        truncated: boolean;
        error: string | null;
      };
    }
  | {
      kind: "xml";
      data: { tree_html: string; raw_html: string; error: string | null };
    }
  | {
      kind: "plist";
      data: {
        entries: Array<{ key: string; kind: string; value: string; depth: number }>;
        error: string | null;
      };
    }
  | { kind: "mermaid_file"; data: { source: string } }
  | { kind: "media"; data: { media_kind: string; url: string } }
  | { kind: "binary"; data: { size: number; likely_type: string } }
  | { kind: "cloud"; data: null }
  | { kind: "error"; data: { message: string } };

type EnvEntry = {
  key: string;
  masked_value: string;
  multiline: boolean;
  secret: boolean;
  comment: boolean;
};

type NotebookCell = {
  cell_type: string;
  html: string;
  outputs: string[];
  execution_count: number | null;
};

type SqliteTable = {
  name: string;
  kind: string;
  columns: Array<{ name: string; col_type: string; pk: boolean; notnull: boolean }>;
  indexes: string[];
  row_count: number;
};

type Envelope = {
  req_id: number;
  path: string;
  name: string;
  size: number;
  preview: Preview;
};

type SettingsShape = { background: string; theme: string };

const headerName = document.querySelector<HTMLElement>("#hv-name")!;
const headerSize = document.querySelector<HTMLElement>("#hv-size")!;
const headerBadge = document.querySelector<HTMLElement>("#hv-badge")!;
const tabsEl = document.querySelector<HTMLElement>("#tabs")!;
const content = document.querySelector<HTMLElement>("#content")!;
const copyBtn = document.querySelector<HTMLButtonElement>("#btn-copy")!;
const openBtn = document.querySelector<HTMLButtonElement>("#btn-open")!;
const closeBtn = document.querySelector<HTMLButtonElement>("#btn-close")!;

let current: Envelope | null = null;
let themeCssLoaded = false;
let activeTab = 0;

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Fallback badge per preview kind when the file name has no known extension. */
const KIND_BADGES: Record<string, { text: string; cls: string }> = {
  text: { text: "TXT", cls: "kind-text" },
  binary: { text: "BIN", cls: "kind-binary" },
  cloud: { text: "CLOUD", cls: "kind-cloud" },
  error: { text: "ERR", cls: "kind-error" },
  markdown: { text: "MD", cls: "kind-md" },
  mermaid_file: { text: "MMD", cls: "kind-mermaid" },
  json_tree: { text: "JSON", cls: "kind-json" },
  json_lines: { text: "JSONL", cls: "kind-json" },
  package_json: { text: "JSON", cls: "kind-json" },
  table: { text: "CSV", cls: "kind-csv" },
  sqlite: { text: "SQLITE", cls: "kind-sqlite" },
  log: { text: "LOG", cls: "kind-log" },
  env: { text: "ENV", cls: "kind-env" },
  sql: { text: "SQL", cls: "kind-sql" },
  notebook: { text: "IPYNB", cls: "kind-notebook" },
  archive: { text: "ARCHIVE", cls: "kind-archive" },
  xml: { text: "XML", cls: "kind-xml" },
  plist: { text: "PLIST", cls: "kind-plist" },
  diff: { text: "DIFF", cls: "kind-diff" },
  code: { text: "CODE", cls: "kind-code" },
  media: { text: "FILE", cls: "kind-media" },
};

const EXT_BADGES: Array<[RegExp, string, string]> = [
  [/\.md$/i, "MD", "kind-md"],
  [/\.jsonc?$|\.jsonl$|\.ndjson$/i, "JSON", "kind-json"],
  [/\.(csv|tsv|tab|psv)$/i, "CSV", "kind-csv"],
  [/\.(sqlite|sqlite3|db)$/i, "SQLITE", "kind-sqlite"],
  [/\.(log|out)$/i, "LOG", "kind-log"],
  [
    /\.(py|rs|ts|tsx|js|jsx|go|java|kt|c|h|cpp|cs|rb|php|lua|sh|ps1|tf|toml|ya?ml|ini|cfg)$/i,
    "CODE",
    "kind-code",
  ],
  [/\.(zip|tar|tgz|gz|7z|rar)$/i, "ARCHIVE", "kind-archive"],
  [/\.(png|jpe?g|gif|webp|bmp|svg|pdf|mp3|wav|mid|mp4|mkv|avi)$/i, "FILE", "kind-media"],
];

function badgeFor(env: Envelope): { text: string; cls: string } {
  for (const [re, text, cls] of EXT_BADGES) {
    if (re.test(env.name)) return { text, cls };
  }
  return KIND_BADGES[env.preview.kind] ?? { text: "FILE", cls: "kind-binary" };
}

function ensureThemeCss(): void {
  if (themeCssLoaded) return;
  themeCssLoaded = true;
  void invoke<string>("code_theme_css").then((css) => {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.appendChild(style);
  });
}

function tabsFor(kind: string): string[] {
  switch (kind) {
    case "markdown":
      return ["Rendered", "Raw"];
    case "json_tree":
      return ["Tree", "Raw"];
    case "sqlite":
      return ["Structure", "Content"];
    case "sql":
      return ["Formatted", "Raw"];
    case "xml":
      return ["Tree", "Raw"];
    case "notebook":
      return ["Content", "Script"];
    default:
      return [];
  }
}

function setTabs(tabs: string[]): void {
  if (tabs.length < 2) {
    tabsEl.replaceChildren();
    tabsEl.hidden = true;
    return;
  }
  tabsEl.hidden = false;
  tabsEl.replaceChildren(
    ...tabs.map((t, i) => {
      const b = document.createElement("button");
      b.className = "tab" + (i === activeTab ? " active" : "");
      b.textContent = t;
      b.addEventListener("click", () => {
        activeTab = i;
        if (current) render(current);
      });
      return b;
    }),
  );
}

function render(env: Envelope): void {
  headerName.textContent = env.name;
  headerName.title = env.path;
  headerSize.textContent = humanSize(env.size);
  const badge = badgeFor(env);
  headerBadge.textContent = badge.text;
  headerBadge.className = `badge ${badge.cls}`;
  copyBtn.disabled = !(env.preview.kind === "text" || env.preview.kind === "markdown");
  openBtn.disabled = false;

  const tabs = tabsFor(env.preview.kind);
  activeTab = Math.min(activeTab, Math.max(tabs.length - 1, 0));
  setTabs(tabs);

  content.classList.add("swapping");
  window.setTimeout(() => {
    renderView(env);
    content.classList.remove("swapping");
    content.classList.remove("entered");
    void content.offsetWidth;
    content.classList.add("entered");
  }, 90);
}

function renderView(env: Envelope): void {
  const p = env.preview;
  if (p.kind === "markdown" && activeTab === 1) {
    showText(p.data.raw, p.data.truncated);
    return;
  }
  if (p.kind === "json_tree" && activeTab === 1) {
    ensureThemeCss();
    const wrap = document.createElement("div");
    wrap.className = "code-block theme-auto";
    wrap.innerHTML = p.data.raw_html;
    content.replaceChildren(wrap);
    return;
  }
  if (p.kind === "sqlite" && activeTab === 1) {
    renderSqliteContent(p.data.tables);
    return;
  }
  if (p.kind === "sql" && activeTab === 1) {
    ensureThemeCss();
    const wrap = document.createElement("div");
    wrap.className = "code-block theme-auto";
    wrap.innerHTML = p.data.raw_html;
    content.replaceChildren(wrap);
    return;
  }
  if (p.kind === "notebook" && activeTab === 1) {
    const pre = document.createElement("pre");
    pre.id = "text-view";
    pre.textContent = p.data.script;
    content.replaceChildren(pre);
    return;
  }
  if (p.kind === "xml" && activeTab === 1) {
    ensureThemeCss();
    const wrap = document.createElement("div");
    wrap.className = "code-block theme-auto";
    wrap.innerHTML = p.data.raw_html;
    content.replaceChildren(wrap);
    return;
  }

  switch (p.kind) {
    case "text":
      showText(p.data.content, p.data.truncated);
      break;
    case "code": {
      ensureThemeCss();
      const frag = document.createDocumentFragment();
      if (p.data.truncated) {
        frag.append(truncBanner("Large file: truncated."));
      }
      const wrap = document.createElement("div");
      wrap.className = "code-block theme-auto";
      wrap.innerHTML = p.data.html;
      frag.append(wrap);
      content.replaceChildren(frag);
      break;
    }
    case "markdown": {
      ensureThemeCss();
      const md = document.createElement("div");
      md.className = "markdown-body";
      md.innerHTML = p.data.html;
      content.replaceChildren(md);
      enhanceMarkdown(md);
      if (p.data.mermaid.length > 0) {
        void renderMermaidBlocks(md, p.data.mermaid);
      }
      void renderMath(md);
      break;
    }
    case "diff": {
      const wrap = document.createElement("div");
      wrap.className = "diff-view";
      wrap.innerHTML = p.data.html;
      content.replaceChildren(wrap);
      break;
    }
    case "package_json": {
      ensureThemeCss();
      const card = document.createElement("div");
      card.className = "info-card pkg-card";
      const s = p.data.summary as {
        name?: string;
        version?: string;
        description?: string;
        dependencies?: number;
        devDependencies?: number;
        scripts?: Array<{ name: string; command: string }>;
        error?: string;
      };
      if (s.error) {
        card.append(line("Error", s.error));
      } else {
        card.append(line("Name", `${s.name ?? ""} ${s.version ?? ""}`.trim() || "(unnamed)"));
        if (s.description) card.append(line("Description", s.description));
        card.append(
          line("Dependencies", `${s.dependencies ?? 0} prod, ${s.devDependencies ?? 0} dev`),
        );
        if (s.scripts && s.scripts.length > 0) {
          const dl = document.createElement("div");
          dl.className = "pkg-scripts";
          for (const sc of s.scripts) {
            const d = document.createElement("div");
            d.className = "pkg-script";
            const n = document.createElement("code");
            n.textContent = sc.name;
            const c = document.createElement("span");
            c.textContent = sc.command;
            c.className = "pkg-script-cmd";
            d.append(n, c);
            dl.append(d);
          }
          card.append(dl);
        }
      }
      const rawWrap = document.createElement("div");
      rawWrap.className = "code-block theme-auto";
      rawWrap.innerHTML = p.data.code_html;
      const outer = document.createDocumentFragment();
      outer.append(card, rawWrap);
      content.replaceChildren(outer);
      break;
    }
    case "json_tree": {
      ensureThemeCss();
      const frag = document.createDocumentFragment();
      if (p.data.truncated) {
        frag.append(truncBanner("Large JSON: showing the first 4,000 nodes."));
      }
      const tree = document.createElement("div");
      tree.className = "jtree";
      tree.innerHTML = p.data.html;
      wireCollapse(tree);
      frag.append(tree);
      content.replaceChildren(frag);
      break;
    }
    case "json_lines": {
      const list = document.createElement("div");
      list.className = "jsonl";
      p.data.lines.forEach((escaped, i) => {
        const row = document.createElement("div");
        row.className = "jsonl-row";
        const no = document.createElement("span");
        no.className = "jsonl-no";
        no.textContent = String(i + 1);
        const body = document.createElement("span");
        body.className = "jsonl-body";
        body.innerHTML = escaped;
        row.append(no, body);
        row.addEventListener("click", () => {
          if (row.dataset["expanded"] === "1" || !current) return;
          row.dataset["expanded"] = "1";
          void invoke<string>("jsonl_record", { path: current.path, line: i })
            .then((html) => {
              const exp = document.createElement("div");
              exp.className = "jsonl-expand";
              const tree = document.createElement("div");
              tree.className = "jtree";
              tree.innerHTML = html;
              wireCollapse(tree);
              exp.append(tree);
              row.after(exp);
            })
            .catch((e) => {
              row.dataset["expanded"] = "0";
              body.textContent = `Expand failed: ${String(e)}`;
            });
        });
        list.append(row);
      });
      content.replaceChildren(list);
      break;
    }
    case "table": {
      renderTable(p.data);
      break;
    }
    case "sqlite": {
      renderSqliteStructure(p.data);
      break;
    }
    case "log": {
      const frag = document.createDocumentFragment();
      const bits: string[] = [
        `${p.data.total_lines.toLocaleString()} lines`,
        `from line ${p.data.first_line_no}`,
      ];
      if (p.data.tail_only) bits.push("last 4 MB");
      if (p.data.truncated) bits.push("showing the last 20,000");
      frag.append(truncBanner(bits.join(", ")));
      const wrap = document.createElement("div");
      wrap.className = "log-view";
      wrap.innerHTML = p.data.html;
      frag.append(wrap);
      const filterBar = document.createElement("div");
      filterBar.className = "log-filterbar";
      const filterBtn = document.createElement("button");
      filterBtn.className = "btn small";
      filterBtn.textContent = "Warnings & errors only";
      filterBtn.addEventListener("click", () => {
        const filtered = filterBtn.dataset["f"] === "1";
        const next = !filtered;
        filterBtn.dataset["f"] = next ? "1" : "0";
        for (const el of wrap.querySelectorAll<HTMLElement>(".log-line")) {
          const cls = el.className;
          el.hidden =
            next &&
            !(cls.includes("log-error") || cls.includes("log-warn") || cls.includes("log-trace"));
        }
        filterBtn.textContent = next ? "Show all lines" : "Warnings & errors only";
      });
      frag.append(filterBar);
      content.replaceChildren(frag);
      break;
    }
    case "env": {
      const card = document.createElement("div");
      card.className = "env-card";
      const bar = document.createElement("div");
      bar.className = "env-bar";
      const revealBtn = document.createElement("button");
      revealBtn.className = "btn small";
      revealBtn.textContent = "Show values";
      bar.append(revealBtn);
      const list = document.createElement("div");
      renderEnvRows(list, p.data.entries, false);
      revealBtn.addEventListener("click", () => {
        const revealed = revealBtn.dataset["r"] === "1";
        if (revealed) {
          renderEnvRows(list, p.data.entries, false);
          revealBtn.textContent = "Show values";
          revealBtn.dataset["r"] = "0";
          return;
        }
        if (!current) return;
        void invoke<Array<[string, string]>>("reveal_env", { path: current.path })
          .then((real) => {
            const map = new Map(real);
            const withReal = p.data.entries.map((e) => ({
              ...e,
              real: map.get(e.key.replaceAll("&amp;", "&")) ?? "",
            }));
            renderEnvRows(list, withReal, true);
            revealBtn.textContent = "Hide values";
            revealBtn.dataset["r"] = "1";
          })
          .catch((e) => {
            revealBtn.textContent = `Reveal failed: ${String(e).slice(0, 60)}`;
          });
      });
      card.append(bar, list);
      content.replaceChildren(card);
      break;
    }
    case "sql": {
      if (p.data.fallback_reason) {
        const card = cardDiv();
        card.append(
          line("Formatted view unavailable", p.data.fallback_reason),
          line("Showing", "Raw source (also on the Raw tab)"),
        );
        content.replaceChildren(card);
        break;
      }
      const wrap = document.createElement("div");
      wrap.className = "sql-view";
      wrap.innerHTML = p.data.formatted_html;
      content.replaceChildren(wrap);
      break;
    }
    case "notebook": {
      if (p.data.error) {
        const card = cardDiv();
        card.append(line("Error", p.data.error));
        content.replaceChildren(card);
        break;
      }
      const wrap = document.createElement("div");
      wrap.className = "notebook-view";
      p.data.cells.forEach((c) => {
        const cell = document.createElement("div");
        cell.className = `nb-cell nb-${c.cell_type}`;
        if (c.execution_count != null) {
          const badge = document.createElement("div");
          badge.className = "nb-exec";
          badge.textContent = `[${c.execution_count}]`;
          cell.append(badge);
        }
        const body = document.createElement("div");
        body.className = "nb-body";
        body.innerHTML = c.html;
        if (c.cell_type === "markdown") void renderMath(body);
        cell.append(body);
        for (const o of c.outputs) {
          const out = document.createElement("div");
          out.className = "nb-output";
          out.innerHTML = o;
          cell.append(out);
        }
        wrap.append(cell);
      });
      content.replaceChildren(wrap);
      break;
    }
    case "archive": {
      if (p.data.error) {
        const card = cardDiv();
        card.append(line("Error", p.data.error));
        content.replaceChildren(card);
        break;
      }
      const frag = document.createDocumentFragment();
      frag.append(
        truncBanner(
          `${p.data.kind}: ${p.data.total_files.toLocaleString()} entries, ${humanSize(p.data.total_bytes)}` +
            (p.data.truncated ? ", listing truncated" : ""),
        ),
      );
      const scroll = document.createElement("div");
      scroll.className = "table-scroll";
      const table = document.createElement("table");
      table.className = "grid";
      const thead = document.createElement("thead");
      const hr = document.createElement("tr");
      ["Path", "Size", "Compressed"].forEach((h) => {
        const th = document.createElement("th");
        th.textContent = h;
        hr.append(th);
      });
      thead.append(hr);
      const tbody = document.createElement("tbody");
      for (const e of p.data.entries) {
        const tr = document.createElement("tr");
        const td1 = document.createElement("td");
        td1.textContent = e.path;
        const td2 = document.createElement("td");
        td2.className = "num";
        td2.textContent = humanSize(e.size);
        const td3 = document.createElement("td");
        td3.className = "num";
        td3.textContent = e.compressed != null ? humanSize(e.compressed) : "";
        tr.append(td1, td2, td3);
        tbody.append(tr);
      }
      table.append(thead, tbody);
      scroll.append(table);
      frag.append(scroll);
      content.replaceChildren(frag);
      break;
    }
    case "xml": {
      const tree = document.createElement("div");
      tree.className = "xml-view";
      tree.innerHTML = p.data.tree_html;
      content.replaceChildren(tree);
      break;
    }
    case "plist": {
      if (p.data.error) {
        const card = cardDiv();
        card.append(line("Error", p.data.error));
        content.replaceChildren(card);
        break;
      }
      const list = document.createElement("div");
      list.className = "plist-view";
      for (const e of p.data.entries) {
        const row = document.createElement("div");
        row.className = "plist-row";
        row.style.paddingLeft = `${8 + e.depth * 20}px`;
        const key = document.createElement("span");
        key.className = "plist-key";
        key.textContent = e.key;
        const kind = document.createElement("span");
        kind.className = "plist-kind";
        kind.textContent = e.kind;
        const val = document.createElement("span");
        val.className = "plist-val";
        val.textContent = e.value;
        row.append(key, kind, val);
        list.append(row);
      }
      content.replaceChildren(list);
      break;
    }
    case "mermaid_file": {
      const holder = document.createElement("div");
      holder.className = "mermaid-file-view";
      const node = document.createElement("div");
      node.className = "winview-mermaid";
      holder.append(node);
      content.replaceChildren(holder);
      void loadMermaid().then((m) =>
        m
          .render(`mmf-${Date.now()}`, p.data.source)
          .then(({ svg }) => {
            node.innerHTML = svg;
            node.classList.add("rendered");
          })
          .catch((e) => {
            node.textContent = `Mermaid error: ${String(e).slice(0, 160)}`;
            node.classList.add("mermaid-error");
          }),
      );
      break;
    }
    case "media": {
      const wrap = document.createElement("div");
      wrap.className = "media-view";
      let el: HTMLElement;
      switch (p.data.media_kind) {
        case "image": {
          const img = document.createElement("img");
          img.src = p.data.url;
          img.className = "media-image";
          el = img;
          break;
        }
        case "video": {
          const v = document.createElement("video");
          v.src = p.data.url;
          v.controls = true;
          v.className = "media-video";
          el = v;
          break;
        }
        case "audio": {
          const a = document.createElement("audio");
          a.src = p.data.url;
          a.controls = true;
          el = a;
          break;
        }
        case "pdf": {
          const embed = document.createElement("embed");
          embed.src = p.data.url;
          embed.type = "application/pdf";
          embed.className = "media-pdf";
          el = embed;
          break;
        }
        default:
          el = cardDiv();
      }
      wrap.append(el);
      content.replaceChildren(wrap);
      break;
    }
    case "binary": {
      const card = cardDiv();
      card.append(
        line("Type", p.data.likely_type),
        line("Size", humanSize(p.data.size)),
        line("Note", "No preview available for this file type."),
      );
      content.replaceChildren(card);
      break;
    }
    case "cloud": {
      const card = cardDiv();
      const msg = document.createElement("div");
      msg.className = "cloud-msg";
      msg.textContent = "Cloud file: this OneDrive placeholder was not downloaded.";
      const hint = document.createElement("div");
      hint.className = "cloud-hint";
      hint.textContent = "Use Open to download and open it in the default app.";
      card.append(msg, hint);
      content.replaceChildren(card);
      break;
    }
    case "error": {
      const card = cardDiv();
      card.append(line("Error", p.data.message));
      content.replaceChildren(card);
      break;
    }
  }
}

function showText(text: string, truncated: boolean): void {
  const frag = document.createDocumentFragment();
  if (truncated) {
    frag.append(truncBanner(`Large file: showing the first ${humanSize(text.length)}.`));
  }
  const pre = document.createElement("pre");
  pre.id = "text-view";
  pre.textContent = text;
  frag.append(pre);
  content.replaceChildren(frag);
}

function truncBanner(msg: string): HTMLDivElement {
  const banner = document.createElement("div");
  banner.className = "trunc-banner";
  banner.textContent = msg;
  return banner;
}

function wireCollapse(root: HTMLElement): void {
  for (const node of root.querySelectorAll<HTMLElement>(".jnode")) {
    if (!node.querySelector(".jchildren")) continue;
    node.classList.add("jparent");
    const brace = node.querySelector(".jbrace");
    brace?.addEventListener("click", (ev) => {
      ev.stopPropagation();
      node.classList.toggle("collapsed");
    });
  }
}

function enhanceMarkdown(md: HTMLElement): void {
  for (const btn of md.querySelectorAll<HTMLElement>(".code-copy")) {
    btn.addEventListener("click", () => {
      const lines = btn.parentElement?.querySelectorAll(".cl");
      const all = lines
        ? Array.from(lines)
            .map((el) => el.textContent ?? "")
            .join("\n")
        : "";
      void writeText(all);
      btn.textContent = "Copied";
      window.setTimeout(() => (btn.textContent = "Copy"), 1200);
    });
  }
}

let mermaidReady: Promise<typeof import("mermaid").default> | null = null;

function loadMermaid(): Promise<typeof import("mermaid").default> {
  mermaidReady ??= import("mermaid").then((m) => {
    m.default.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: isDark() ? "dark" : "default",
    });
    return m.default;
  });
  return mermaidReady;
}

async function renderMermaidBlocks(root: HTMLElement, sources: string[]): Promise<void> {
  const mermaid = await loadMermaid();
  const nodes = root.querySelectorAll<HTMLElement>(".winview-mermaid[data-token]");
  for (const node of nodes) {
    const token = Number(node.dataset["token"] ?? 0);
    const src = sources[token];
    if (src == null) continue;
    try {
      const { svg } = await mermaid.render(`mv-${token}-${Date.now()}`, src);
      node.innerHTML = svg;
      node.classList.add("rendered");
    } catch (e) {
      node.textContent = `Mermaid error: ${String(e).slice(0, 120)}`;
      node.classList.add("mermaid-error");
    }
  }
}

async function renderMath(root: HTMLElement): Promise<void> {
  try {
    const auto: (el: HTMLElement, opts: Record<string, unknown>) => void = (
      await import("katex/dist/contrib/auto-render.mjs")
    ) as (el: HTMLElement, opts: Record<string, unknown>) => void;
    // comrak's math extension already emits `<span data-math-style="...">`
    // with the TeX and no delimiters, so auto-render can't see them.
    const katex = (await import("katex")).default;
    root.querySelectorAll<HTMLElement>("[data-math-style]").forEach((el) => {
      katex.render(el.textContent ?? "", el, {
        displayMode: el.dataset["mathStyle"] === "display",
        throwOnError: false,
      });
    });
    auto(root, {
      delimiters: [
        { left: "$$", right: "$$", display: true },
        { left: "$", right: "$", display: false },
        { left: "\\(", right: "\\)", display: false },
        { left: "\\[", right: "\\]", display: true },
      ],
      throwOnError: false,
    });
  } catch {
    // math stays as plain text when KaTeX fails to load
  }
}

function isDark(): boolean {
  const t = document.documentElement.dataset["theme"];
  if (t === "dark") return true;
  if (t === "light") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function cardDiv(): HTMLDivElement {
  const d = document.createElement("div");
  d.className = "info-card";
  return d;
}

function line(label: string, value: string): HTMLDivElement {
  const d = document.createElement("div");
  d.className = "info-line";
  const l = document.createElement("span");
  l.className = "info-label";
  l.textContent = label;
  const v = document.createElement("span");
  v.className = "info-value";
  v.textContent = value;
  d.append(l, v);
  return d;
}

let tableState: { batch: number; total: number; loading: boolean } | null = null;

function renderTable(d: {
  columns: string[];
  numeric: boolean[];
  total_rows: number;
  sample_rows: string[][];
  decimal_comma: boolean;
  truncated: boolean;
}): void {
  tableState = { batch: 0, total: d.total_rows, loading: false };
  const frag = document.createDocumentFragment();
  frag.append(
    truncBanner(
      `${d.total_rows.toLocaleString()} rows, first ${d.sample_rows.length} shown` +
        (d.truncated ? ", truncated at 200,000" : ""),
    ),
  );
  const scroll = document.createElement("div");
  scroll.className = "table-scroll";
  const table = document.createElement("table");
  table.className = "grid";
  const thead = document.createElement("thead");
  const hr = document.createElement("tr");
  d.columns.forEach((c, i) => {
    const th = document.createElement("th");
    th.textContent = c;
    if (d.numeric[i]) th.classList.add("num");
    hr.append(th);
  });
  thead.append(hr);
  const tbody = document.createElement("tbody");
  appendRows(tbody, d.sample_rows, d.numeric);
  table.append(thead, tbody);
  scroll.append(table);
  frag.append(scroll);
  content.replaceChildren(frag);

  scroll.addEventListener("scroll", () => {
    if (!tableState || tableState.loading || !current) return;
    if (scroll.scrollTop + scroll.clientHeight < scroll.scrollHeight - 200) return;
    const loaded = tbody.childElementCount;
    if (loaded >= tableState.total) return;
    tableState.loading = true;
    const next = tableState.batch + 1;
    const onRows = new Channel<string[][]>();
    onRows.onmessage = (rows) => {
      tableState!.batch = next;
      appendRows(tbody, rows, d.numeric);
      tableState!.loading = false;
    };
    void invoke("table_rows", { path: current.path, batchIndex: next, onRows }).catch(() => {
      tableState!.loading = false;
    });
  });
}

function appendRows(tbody: HTMLTableSectionElement, rows: string[][], numeric: boolean[]): void {
  for (const row of rows) {
    const tr = document.createElement("tr");
    row.forEach((cell, i) => {
      const td = document.createElement("td");
      td.textContent = cell;
      if (numeric[i]) td.classList.add("num");
      tr.append(td);
    });
    tbody.append(tr);
  }
}

let sqliteSelected: string | null = null;

function renderSqliteStructure(d: {
  tables: SqliteTable[];
  wal_warning: boolean;
  error: string | null;
}): void {
  if (d.error) {
    const card = cardDiv();
    card.append(line("Error", d.error));
    content.replaceChildren(card);
    return;
  }
  const wrap = document.createElement("div");
  wrap.className = "sqlite-view";
  if (d.wal_warning) {
    wrap.append(
      truncBanner("Warning: a non-empty -wal file exists; the database may have uncommitted state."),
    );
  }
  const list = document.createElement("div");
  list.className = "sqlite-tables";
  for (const t of d.tables) {
    const item = document.createElement("button");
    item.className = "sqlite-table-item" + (t.name === sqliteSelected ? " selected" : "");
    item.innerHTML = `<span class="st-name"></span><span class="st-meta"></span>`;
    item.querySelector<HTMLElement>(".st-name")!.textContent = t.name;
    item.querySelector<HTMLElement>(".st-meta")!.textContent =
      `${t.kind}, ${t.row_count.toLocaleString()} rows`;
    item.addEventListener("click", () => {
      sqliteSelected = t.name;
      renderSqliteStructure(d);
    });
    list.append(item);
  }
  wrap.append(list);
  const detail = document.createElement("div");
  detail.className = "sqlite-detail";
  const sel = d.tables.find((t) => t.name === sqliteSelected) ?? d.tables[0];
  if (sel) {
    const head = document.createElement("div");
    head.className = "sqlite-head";
    head.textContent = sel.name;
    const t2 = document.createElement("table");
    t2.className = "grid";
    const tr1 = document.createElement("tr");
    ["Column", "Type", "PK", "Not null"].forEach((h) => {
      const th = document.createElement("th");
      th.textContent = h;
      tr1.append(th);
    });
    const tb = document.createElement("tbody");
    for (const c of sel.columns) {
      const tr = document.createElement("tr");
      [c.name, c.col_type || "(none)", c.pk ? "PK" : "", c.notnull ? "not null" : ""].forEach(
        (v) => {
          const td = document.createElement("td");
          td.textContent = v;
          tr.append(td);
        },
      );
      tb.append(tr);
    }
    t2.append(tr1, tb);
    const idx = document.createElement("div");
    idx.className = "sqlite-indexes";
    idx.textContent = sel.indexes.length ? `Indexes: ${sel.indexes.join(", ")}` : "No indexes";
    detail.append(head, t2, idx);
  }
  wrap.append(detail);
  content.replaceChildren(wrap);
}

function renderSqliteContent(tables: SqliteTable[]): void {
  const sel = tables.find((t) => t.name === sqliteSelected) ?? tables[0];
  if (!sel) {
    content.replaceChildren(cardDiv());
    return;
  }
  const wrap = document.createElement("div");
  wrap.className = "sqlite-content";
  wrap.append(truncBanner(`${sel.name}: first 1,000 rows (read-only)`));
  const holder = document.createElement("div");
  holder.className = "table-scroll";
  wrap.append(holder);
  content.replaceChildren(wrap);
  if (!current) return;
  void invoke<[string[][], number]>("sqlite_rows", {
    path: current.path,
    table: sel.name,
    offset: 0,
    limit: 1000,
  })
    .then(([rows]) => {
      const table = document.createElement("table");
      table.className = "grid";
      const tb = document.createElement("tbody");
      appendRows(tb, rows, sel.columns.map(() => false));
      table.append(tb);
      holder.append(table);
    })
    .catch((e) => {
      holder.textContent = `Error: ${String(e)}`;
    });
}

function renderEnvRows(
  list: HTMLElement,
  entries: Array<EnvEntry & { real?: string }>,
  revealed: boolean,
): void {
  list.replaceChildren(
    ...entries.map((e) => {
      const row = document.createElement("div");
      row.className = "env-row" + (e.comment ? " env-comment" : "");
      const key = document.createElement("span");
      key.className = "env-key";
      key.textContent = e.key;
      row.append(key);
      if (e.comment) return row;
      const val = document.createElement("span");
      val.className = "env-val";
      val.textContent = revealed && e.real ? e.real : e.masked_value;
      row.append(val);
      if (e.multiline) {
        const tag = document.createElement("span");
        tag.className = "env-tag";
        tag.textContent = "multiline";
        row.append(tag);
      }
      if (e.secret) {
        const tag = document.createElement("span");
        tag.className = "env-tag secret";
        tag.textContent = "SECRET";
        row.append(tag);
      }
      return row;
    }),
  );
}

function applyBackground(s: SettingsShape | undefined): void {
  if (!s) return;
  const bg = s.background;
  if (bg === "mica") {
    document.body.classList.add("bg-mica");
    document.body.style.removeProperty("background-color");
  } else if (bg === "light") {
    document.body.classList.remove("bg-mica");
    document.body.style.setProperty("background-color", "#f5f5f5");
  } else if (bg === "dark") {
    document.body.classList.remove("bg-mica");
    document.body.style.setProperty("background-color", "#1f1f1f");
  } else if (/^#[0-9a-fA-F]{6}$/.test(bg)) {
    document.body.classList.remove("bg-mica");
    document.body.style.setProperty("background-color", bg);
  }
}

void listen<Envelope>("preview:load", (e) => {
  current = e.payload;
  activeTab = 0;
  render(current);
  document.body.classList.remove("shown");
  void document.body.offsetWidth;
  document.body.classList.add("shown");
});

void listen<SettingsShape>("settings:changed", (e) => {
  applyBackground(e.payload);
  document.documentElement.dataset["theme"] = e.payload.theme;
  mermaidReady = null;
});

copyBtn.addEventListener("click", () => {
  if (current?.preview.kind === "text") {
    void writeText(current.preview.data.content);
  } else if (current?.preview.kind === "markdown") {
    void writeText(current.preview.data.raw);
  }
});

openBtn.addEventListener("click", () => {
  if (current) void invoke("open_default", { path: current.path });
});

closeBtn.addEventListener("click", () => {
  void invoke("close_preview");
});

document.querySelector<HTMLElement>("#header")!.setAttribute("data-tauri-drag-region", "");
