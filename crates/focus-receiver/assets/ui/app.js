// SPDX-License-Identifier: Apache-2.0
// The statistics and configuration page. No dependencies. Every piece of text that comes
// from outside (app names, paths, error messages) is set with textContent, never parsed as
// markup, and the page's Content-Security-Policy forbids inline script and style anyway.
"use strict";

(() => {
  const TOKEN_KEY = "focus-autopilot-token";
  const REFRESH_MS = 30000;

  const $ = (id) => document.getElementById(id);

  /** Creates an element. `props` may hold class, text, and attributes; `on` holds listeners. */
  function h(tag, props, ...kids) {
    const el = document.createElement(tag);
    for (const [name, value] of Object.entries(props || {})) {
      if (value === undefined || value === null || value === false) continue;
      if (name === "class") el.className = value;
      else if (name === "text") el.textContent = value;
      else if (name === "on") {
        for (const [event, fn] of Object.entries(value)) el.addEventListener(event, fn);
      } else el.setAttribute(name, value === true ? "" : String(value));
    }
    for (const kid of kids.flat(Infinity)) {
      if (kid === undefined || kid === null || kid === false) continue;
      el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
    }
    return el;
  }

  // ---------- the key that lets this page talk to focusd ----------

  function store(action, value) {
    try {
      if (action === "get") return window.sessionStorage.getItem(value);
      window.sessionStorage.setItem(value[0], value[1]);
    } catch (_) {
      /* storage can be unavailable; the page still works for this load */
    }
    return null;
  }

  let token = store("get", TOKEN_KEY);

  /** Takes the key from the address, if it is there, and removes it from the address bar. */
  function adoptKey() {
    const fromHash = new URLSearchParams(window.location.hash.slice(1)).get("t");
    if (!fromHash) return false;
    token = fromHash;
    store("set", [TOKEN_KEY, fromHash]);
    window.history.replaceState(null, "", window.location.pathname);
    return true;
  }

  adoptKey();

  class Refused extends Error {}

  async function api(path, body) {
    let response;
    try {
      response = await fetch(path, {
        method: body ? "POST" : "GET",
        headers: Object.assign(
          { "X-Focus-Token": token || "" },
          body ? { "Content-Type": "application/json" } : {}
        ),
        body: body ? JSON.stringify(body) : undefined,
        cache: "no-store",
      });
    } catch (_) {
      throw new Error("Can't reach Focus Autopilot. Is focusd still running?");
    }
    if (response.status === 401 || response.status === 403) throw new Refused();
    const data = await response.json().catch(() => null);
    if (!response.ok) {
      throw new Error((data && data.error) || `the request failed (${response.status})`);
    }
    return data;
  }

  function showNotice(text) {
    const notice = $("notice");
    notice.textContent = text;
    notice.hidden = !text;
  }

  function explain(error) {
    if (error instanceof Refused) {
      showNotice(
        "This page needs its key. Open it by running `focusctl ui` in a terminal, which " +
          "hands it the key to talk to Focus Autopilot on this computer."
      );
    } else {
      showNotice(error.message);
    }
  }

  // ---------- small helpers ----------

  /** 45 s, 12 min, 1 h 12 min: the same wording the insights use. */
  function human(secs) {
    if (secs < 60) return `${secs} s`;
    const minutes = Math.floor((secs + 30) / 60);
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    if (hours === 0) return `${rest} min`;
    return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
  }

  function plural(n, noun) {
    return n === 1 ? `1 ${noun}` : `${n} ${noun}s`;
  }

  function longDate(day) {
    const [y, m, d] = day.date.split("-").map(Number);
    return new Date(y, m - 1, d).toLocaleDateString(undefined, {
      weekday: "short",
      day: "numeric",
      month: "short",
    });
  }

  // ---------- tooltip: enhances, never gates; the same on hover and on keyboard focus ----------

  const tooltip = $("tooltip");

  function showTip(target, content, pointer) {
    tooltip.replaceChildren(...content);
    tooltip.hidden = false;
    const box = target.getBoundingClientRect();
    const tip = tooltip.getBoundingClientRect();
    let x = pointer ? pointer.clientX + 14 : box.left + box.width / 2 - tip.width / 2;
    let y = pointer ? pointer.clientY + 16 : box.top - tip.height - 8;
    x = Math.max(8, Math.min(x, window.innerWidth - tip.width - 8));
    if (y < 8) y = box.bottom + 8;
    y = Math.min(y, window.innerHeight - tip.height - 8);
    tooltip.style.left = `${x}px`;
    tooltip.style.top = `${y}px`;
  }

  function hideTip() {
    tooltip.hidden = true;
  }

  function withTip(el, content) {
    el.addEventListener("pointermove", (event) => showTip(el, content(), event));
    el.addEventListener("pointerleave", hideTip);
    el.addEventListener("focus", () => showTip(el, content(), null));
    el.addEventListener("blur", hideTip);
    return el;
  }

  function tipRow(value, label, key) {
    return h(
      "div",
      { class: "t-row" },
      key ? h("span", { class: `t-key ${key}` }) : null,
      h("strong", { text: value }),
      h("span", { text: label })
    );
  }

  // ---------- statistics ----------

  let stats = null;
  let statsText = "";
  let range = store("get", "focus-autopilot-range") === "week" ? "week" : "today";
  const asTable = { trend: false, apps: false };

  /** Tick marks for a duration axis: at most four steps of a round size. */
  function axis(maxSecs) {
    const steps = [10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 14400, 21600, 43200, 86400];
    const step = steps.find((s) => maxSecs / s <= 4) || 86400;
    const top = Math.max(step, Math.ceil(maxSecs / step) * step);
    const ticks = [];
    for (let t = 0; t <= top; t += step) ticks.push(t);
    return { top, ticks };
  }

  function tableToggle(which) {
    return h("button", {
      class: "btn quiet",
      type: "button",
      "aria-pressed": String(asTable[which]),
      text: asTable[which] ? "Show as chart" : "Show as table",
      on: {
        click: () => {
          asTable[which] = !asTable[which];
          renderStats();
        },
      },
    });
  }

  function dataTable(head, rows) {
    return h(
      "div",
      { class: "table-wrap" },
      h(
        "table",
        { class: "data" },
        h("thead", null, h("tr", null, head.map((text) => h("th", { scope: "col", text })))),
        h("tbody", null, rows.map((row) => h("tr", null, row.map((text) => h("td", { text })))))
      )
    );
  }

  function insightsCard(s) {
    return h(
      "section",
      { class: "card" },
      h("h2", { text: "What stands out" }),
      h("ul", { class: "insights" }, s.insights.map((text) => h("li", { text })))
    );
  }

  function trendCard(s) {
    const days = s.days;
    const max = Math.max(...days.map((d) => d.claude_s));
    const head = h(
      "div",
      { class: "card-head" },
      h(
        "div",
        null,
        h("h2", { text: "Time Claude spent working, last 7 days" }),
        h("p", { class: "sub", text: "Each column is a day. Hover or focus a column for its numbers." })
      ),
      max > 0 ? tableToggle("trend") : null
    );
    if (max === 0) {
      return h("section", { class: "card" }, head, h("p", { class: "empty", text: "No runs in the last 7 days." }));
    }
    if (asTable.trend) {
      return h(
        "section",
        { class: "card" },
        head,
        dataTable(
          ["Day", "Prompts", "Claude worked", "You, elsewhere meanwhile", "You at the keyboard"],
          days.map((d) => [longDate(d), String(d.prompts), human(d.claude_s), human(d.away_s), human(d.tracked_s)])
        )
      );
    }
    const { top, ticks } = axis(max);
    const yAxis = h("div", { class: "cols-y", "aria-hidden": "true" });
    const plot = h("div", { class: "cols-plot" });
    for (const tick of ticks) {
      const at = `${(tick / top) * 100}%`;
      const label = h("span", { text: tick === 0 ? "0" : human(tick) });
      label.style.bottom = at;
      yAxis.append(label);
      if (tick > 0) {
        const line = h("div", { class: "gridline" });
        line.style.bottom = at;
        plot.append(line);
      }
    }
    const bars = h("div", { class: "cols-bars" });
    const last = days.length - 1;
    days.forEach((day, i) => {
      const height = `${(day.claude_s / top) * 100}%`;
      const bar = h("span", { class: "bar" });
      bar.style.height = height;
      const col = h(
        "button",
        {
          class: "col",
          type: "button",
          "aria-label": `${longDate(day)}: Claude worked ${human(day.claude_s)} across ${plural(day.prompts, "prompt")}`,
        },
        bar
      );
      // Label sparingly: the tallest day and today. The rest is in the tooltip and the table.
      if (day.claude_s > 0 && (day.claude_s === max || i === last)) {
        const cap = h("span", { class: "cap", text: human(day.claude_s) });
        cap.style.bottom = height;
        col.append(cap);
      }
      withTip(col, () => [
        h("div", { class: "t-title", text: longDate(day) }),
        tipRow(human(day.claude_s), "Claude working"),
        tipRow(String(day.prompts), day.prompts === 1 ? "prompt" : "prompts"),
        day.away_s > 0 ? tipRow(human(day.away_s), "you, elsewhere meanwhile") : null,
      ]);
      bars.append(col);
    });
    plot.append(bars);
    const xAxis = h(
      "div",
      { class: "cols-x", "aria-hidden": "true" },
      days.map((d, i) => h("span", { class: i === last ? "today" : null, text: i === last ? "Today" : d.label }))
    );
    return h("section", { class: "card" }, head, h("div", { class: "cols" }, yAxis, plot, xAxis));
  }

  function filterRow() {
    const option = (value, text) =>
      h("button", {
        type: "button",
        "aria-pressed": String(range === value),
        text,
        on: {
          click: () => {
            range = value;
            store("set", ["focus-autopilot-range", value]);
            renderStats();
          },
        },
      });
    return h(
      "div",
      { class: "filter-row" },
      h("span", { class: "label", id: "range-label", text: "Show" }),
      h(
        "div",
        { class: "segmented", role: "group", "aria-labelledby": "range-label" },
        option("today", "Today"),
        option("week", "Last 7 days")
      )
    );
  }

  function tile(label, value, note) {
    return h(
      "div",
      { class: "tile" },
      h("div", { class: "label", text: label }),
      h("div", { class: "value", text: value }),
      h("div", { class: "note", text: note || "" })
    );
  }

  function tiles(t) {
    return h(
      "div",
      { class: "tiles" },
      tile("Prompts", String(t.prompts), t.needed_input > 0 ? `${t.needed_input} stopped to ask you` : ""),
      tile("Claude worked", human(t.claude_s), t.longest_run_s > 0 ? `longest run ${human(t.longest_run_s)}` : ""),
      tile(
        "You, elsewhere meanwhile",
        human(t.away_s),
        t.home_s > 0 ? `${human(t.home_s)} in the Claude window` : ""
      ),
      tile(
        "Back when called",
        t.reaction_avg_s === null ? "–" : human(Math.max(1, t.reaction_avg_s)),
        t.reactions > 0 ? `average of ${plural(t.reactions, "return")}` : "no returns yet"
      ),
      tile("App switches", String(t.switches), t.tracked_s > 0 ? `${human(t.tracked_s)} at the keyboard` : "")
    );
  }

  function appsCard(s) {
    const apps = range === "today" ? s.apps_today : s.apps_week;
    const when = range === "today" ? "today" : "in the last 7 days";
    const head = h(
      "div",
      { class: "card-head" },
      h(
        "div",
        null,
        h("h2", { text: `Where your time went, ${range === "today" ? "today" : "last 7 days"}` }),
        h("p", {
          class: "sub",
          text: "Time each app was in front while you were at the keyboard, longest first.",
        })
      ),
      apps.length ? tableToggle("apps") : null
    );
    if (!apps.length) {
      const why = s.tracking
        ? `No app time recorded ${when}.`
        : "Recording time per app is turned off. You can turn it on under Configuration.";
      return h("section", { class: "card" }, head, h("p", { class: "empty", text: why }));
    }
    if (asTable.apps) {
      return h(
        "section",
        { class: "card" },
        head,
        dataTable(
          ["App", "While Claude worked", "Rest of the time", "Total"],
          apps.map((a) => [
            a.home ? `${a.name} (where you prompt)` : a.name,
            human(a.claude_s),
            human(a.s - a.claude_s),
            human(a.s),
          ])
        )
      );
    }
    const legend = h(
      "div",
      { class: "legend" },
      h("span", { class: "key" }, h("span", { class: "swatch s1" }), "While Claude worked"),
      h("span", { class: "key" }, h("span", { class: "swatch s2" }), "Rest of the time")
    );
    const max = Math.max(...apps.map((a) => a.s));
    const rows = apps.map((a) => {
      const rest = a.s - a.claude_s;
      const fill = h("div", { class: "fill" });
      fill.style.width = `${Math.max(1, (a.s / max) * 100)}%`;
      for (const [cls, secs] of [["s1", a.claude_s], ["s2", rest]]) {
        if (secs <= 0) continue;
        const seg = h("span", { class: `seg ${cls}` });
        seg.style.flexGrow = String(secs);
        fill.append(seg);
      }
      const row = h(
        "button",
        {
          class: "app-row",
          type: "button",
          "aria-label": `${a.name}: ${human(a.s)} in total, ${human(a.claude_s)} of it while Claude worked`,
        },
        h(
          "span",
          { class: "app-name", title: a.id },
          h("span", { class: "app-label", text: a.name }),
          a.home ? h("span", { class: "tag", text: "where you prompt" }) : null
        ),
        h("span", { class: "app-bar", "aria-hidden": "true" }, fill),
        h("span", { class: "app-total", text: human(a.s) })
      );
      return withTip(row, () => [
        h("div", { class: "t-title", text: a.name }),
        tipRow(human(a.s), "in total"),
        tipRow(human(a.claude_s), "while Claude worked", "s1"),
        tipRow(human(rest), "rest of the time", "s2"),
      ]);
    });
    return h("section", { class: "card" }, head, legend, h("div", { class: "apps" }, rows));
  }

  function statsFooter(s) {
    const clear = h("button", { class: "btn quiet danger", type: "button", text: "Clear statistics…" });
    const actions = h("span", null, clear);
    clear.addEventListener("click", () => {
      const confirm = h("button", { class: "btn danger", type: "button", text: "Yes, erase all of it" });
      const cancel = h("button", { class: "btn quiet", type: "button", text: "Cancel" });
      cancel.addEventListener("click", () => renderStats());
      confirm.addEventListener("click", async () => {
        $("stats-root").classList.add("stale");
        try {
          stats = await api(`/api/stats/clear?tz=${-new Date().getTimezoneOffset()}`, {});
          statsText = JSON.stringify(stats);
          showNotice("");
        } catch (error) {
          explain(error);
        }
        $("stats-root").classList.remove("stale");
        renderStats();
      });
      actions.replaceChildren("Erase recorded app time and run history? ", confirm, " ", cancel);
      confirm.focus();
    });
    return h(
      "div",
      { class: "foot" },
      h("span", {
        text: s.tracking
          ? "Only app names and durations are recorded, and they never leave this computer."
          : "Recording time per app is off.",
      }),
      actions
    );
  }

  function renderStats() {
    if (!stats) return;
    hideTip();
    const root = $("stats-root");
    root.setAttribute("aria-busy", "false");
    root.replaceChildren(
      insightsCard(stats),
      trendCard(stats),
      filterRow(),
      tiles(stats[range]),
      appsCard(stats),
      statsFooter(stats)
    );
    const labels = {
      idle: "Idle",
      running: "Claude is working",
      away: "Claude is working, you're elsewhere",
      paused: "Paused",
    };
    const status = $("status");
    status.textContent = labels[stats.status] || "";
    status.hidden = !status.textContent;
  }

  async function loadStats(force) {
    // Don't pull the page out from under someone reading a tooltip or tabbing through it.
    if (!force && stats && $("stats-root").contains(document.activeElement)) return;
    try {
      const fresh = await api(`/api/stats?tz=${-new Date().getTimezoneOffset()}`);
      const text = JSON.stringify(fresh);
      showNotice("");
      if (text === statsText) return;
      stats = fresh;
      statsText = text;
      renderStats();
    } catch (error) {
      explain(error);
    }
  }

  // ---------- configuration ----------

  const SECONDS = { unit: "s" };
  const GROUPS = [
    {
      title: "General",
      items: [
        {
          key: "enabled",
          type: "switch",
          label: "Focus Autopilot is on",
          help: "Turn off to stop every toast and switch without quitting.",
        },
        {
          key: "switching.return_on_needs_input",
          type: "switch",
          label: "Call me back when Claude needs approval or asks a question",
          help: "Off: only when it finishes.",
        },
        { key: "ui.sound_on_return", type: "switch", label: "Play a sound when Claude calls me back" },
        {
          key: "switching.auto_return",
          type: "switch",
          label: "Bring me back by itself after a countdown",
          help:
            "When Claude is done or needs you and you are in another app, a box counts down and then the window " +
            "you prompted from comes forward. The focus key snoozes it. Off: the box waits, and the focus key " +
            "takes you back.",
        },
        {
          key: "countdown.return_seconds",
          type: "number",
          label: "Countdown before bringing me back",
          help: "Can't be 0: every automatic switch is announced.",
          min: 1,
          max: 60,
          ...SECONDS,
          showIf: (v) => v["switching.auto_return"],
        },
        {
          key: "countdown.snooze_seconds",
          type: "number",
          label: "A snooze lasts",
          min: 5,
          max: 3600,
          ...SECONDS,
          showIf: (v) => v["switching.auto_return"],
        },
        {
          key: "countdown.max_snoozes",
          type: "number",
          label: "Snoozes allowed per return",
          min: 0,
          max: 20,
          unit: "",
          showIf: (v) => v["switching.auto_return"],
        },
        {
          key: "stats.track_app_time",
          type: "switch",
          label: "Record time per app for the Statistics tab",
          help: "App names and durations only, kept on this computer. Never window titles or anything you type.",
        },
      ],
    },
    {
      title: "Recap",
      intro: "The card you see when you come back to Claude.",
      items: [
        {
          key: "recap.mode",
          type: "choice-switch",
          on: "llm",
          off: "basic",
          label: "Summarise with Claude (LLM recap)",
          help:
            "Your own claude command writes three short lines: what you asked, what Claude did, what it needs. " +
            "It is sent your prompt, the commands and file names from the run and the end of Claude's last " +
            "message, with likely secrets redacted, and it uses a little of your Claude usage each time. " +
            "Off: the card shows your prompt and the start of Claude's reply.",
        },
        {
          key: "recap.llm_timeout_seconds",
          type: "number",
          label: "Wait for the summary up to",
          help: "After this the plain card is shown instead.",
          min: 1,
          max: 120,
          ...SECONDS,
          showIf: (v) => v["recap.mode"] === "llm",
        },
        { key: "ui.recap_seconds", type: "number", label: "Keep the recap card on screen for", min: 1, max: 120, ...SECONDS },
      ],
    },
    {
      title: "Switching away",
      intro:
        "When a prompt runs long, Focus Autopilot can move you to another app, after a countdown you can cancel. " +
        "It goes to the app you were last called back from, or else to an away app you name here. With neither, " +
        "nothing switches you away.",
      items: [
        {
          key: "away.resume_last_app",
          type: "switch",
          label: "After my next prompt, send me back to the app I was called back from",
          help: "Where you left off comes before the away apps below. Off: only the away apps are used.",
        },
        {
          key: "away.apps",
          type: "apps",
          label: "Away apps",
          help:
            "Where to go when there is nowhere you left off, such as on the first prompt. The first one in the " +
            "list that is running is used. An app you prompt Claude from can't be an away app.",
        },
        {
          key: "away.immersive",
          type: "subset",
          of: "away.apps",
          label: "Hard to leave",
          help: "These get a longer countdown with a warning, for apps like games.",
        },
        { key: "switching.away_after_seconds", type: "number", label: "Switch away after a prompt has run for", min: 0, max: 3600, ...SECONDS },
        {
          key: "switching.away_after_seconds_when_long",
          type: "number",
          label: "…or this soon when the run looks like a long one",
          min: 0,
          max: 3600,
          ...SECONDS,
        },
        {
          key: "switching.short_grace_seconds",
          type: "number",
          label: "…and wait this long when it looks like a short one",
          min: 0,
          max: 3600,
          ...SECONDS,
        },
        {
          key: "switching.away_requires_idle_seconds",
          type: "number",
          label: "Only once I haven't touched the keyboard or mouse for",
          help: "0: no waiting. The switch itself always waits for a pause in your typing.",
          min: 0,
          max: 600,
          ...SECONDS,
        },
        {
          key: "countdown.seconds",
          type: "number",
          label: "Countdown before switching away",
          help: "Can't be 0: every automatic switch is announced.",
          min: 1,
          max: 60,
          ...SECONDS,
        },
        {
          key: "countdown.away_prompt_seconds",
          type: "number",
          label: "Keep the “switch away?” prompt up for",
          min: 5,
          max: 600,
          ...SECONDS,
        },
      ],
    },
    {
      title: "Home apps",
      intro:
        "The apps you prompt Claude from, which is where the key takes you back to. Common terminals and editors " +
        "are built in. Add any other app you run Claude Code in.",
      items: [{ key: "home.extra_apps", type: "apps", label: "Extra home apps" }],
    },
    {
      title: "Hotkeys",
      intro:
        "Write a key as words joined by +, for example Control+Option+Space. It needs at least one of Control, " +
        "Option, Shift or Command. Leave a field empty to turn that key off. A change takes effect the next time " +
        "focusd starts.",
      items: [
        { key: "hotkeys.focus_key", type: "text", label: "Act on the box on screen (go back, switch now, snooze)" },
        { key: "hotkeys.pause", type: "text", label: "Pause or resume" },
        { key: "hotkeys.declare_long", type: "text", label: "This run will take a while: switch me away now" },
      ],
    },
    {
      title: "Advanced",
      collapsed: true,
      items: [
        {
          key: "switching.force_foreground",
          type: "switch",
          label: "Windows: retry a refused switch after one empty mouse event",
          help:
            "Windows only lets a program bring a window forward if it received the last input. The empty event " +
            "(no movement, no click) gives Focus Autopilot that standing. Off: such a switch becomes a question " +
            "and the focus key does it. Does nothing on a Mac. Takes effect when focusd restarts.",
        },
        { key: "switching.input_quiet_ms", type: "number", label: "Before switching, wait for a pause in typing of", min: 0, max: 10000, unit: "ms" },
        { key: "switching.input_quiet_max_seconds", type: "number", label: "…but ask instead after waiting", min: 0, max: 60, ...SECONDS },
        {
          key: "switching.debounce_seconds",
          type: "number",
          label: "Don't flip straight back if Claude finishes within this long of switching away",
          min: 0,
          max: 600,
          ...SECONDS,
        },
        {
          key: "switching.needs_input_debounce_seconds",
          type: "number",
          label: "Ignore an approval request that clears itself within",
          min: 0,
          max: 60,
          ...SECONDS,
        },
        {
          key: "switching.resume_idle_seconds",
          type: "number",
          label: "After I answer Claude, allow switching away again once idle for",
          min: 0,
          max: 600,
          ...SECONDS,
        },
        { key: "switching.stale_after_seconds", type: "number", label: "Treat a run with no activity as over after", min: 60, max: 86400, ...SECONDS },
        { key: "switching.verify_after_ms", type: "number", label: "Check that a switch worked after", min: 50, max: 5000, unit: "ms" },
        { key: "history.retention_days", type: "number", label: "Keep statistics and run history for", min: 1, max: 3650, unit: "days" },
        {
          key: "ignore.cwd_globs",
          type: "list",
          label: "Ignore Claude sessions in these folders",
          help: "Folder patterns such as /Users/me/scratch/**. * stays within one folder name, ** crosses folders.",
          placeholder: "/path/to/folder/**",
        },
      ],
    },
  ];

  let config = null;

  function setSaveState(text, kind) {
    const el = $("save-state");
    el.textContent = text;
    el.className = `save-state${kind ? ` ${kind}` : ""}`;
  }

  async function save(changes) {
    setSaveState("Saving…");
    const focused = document.activeElement && document.activeElement.dataset.key;
    try {
      config = await api("/api/config", { values: changes });
      showNotice("");
      const restart = config.restart_required || [];
      setSaveState(
        restart.length ? "Saved. Restart focusd for the hotkey change to take effect." : "Saved.",
        "ok"
      );
    } catch (error) {
      if (error instanceof Refused) explain(error);
      setSaveState(`Not saved: ${error.message || "this page has lost its key"}.`, "err");
    }
    // Redraw from what the app now holds: a refused change snaps back to the real value.
    renderConfig();
    if (focused) {
      const again = document.querySelector(`[data-key="${CSS.escape(focused)}"]`);
      if (again) again.focus();
    }
  }

  function appName(id) {
    const app = (config.running_apps || []).find((a) => a.id === id);
    return app ? app.name : id;
  }

  function chips(item, values, onRemove) {
    return h(
      "div",
      { class: "chips" },
      values.length
        ? values.map((value) =>
            h(
              "span",
              { class: "chip", title: value },
              h("span", { text: item.type === "apps" ? appName(value) : value }),
              h("button", {
                type: "button",
                "aria-label": `Remove ${item.type === "apps" ? appName(value) : value}`,
                text: "×",
                on: { click: () => onRemove(value) },
              })
            )
          )
        : h("span", { class: "help", text: "None." })
    );
  }

  function textAdder(item, placeholder, onAdd) {
    const input = h("input", {
      type: "text",
      placeholder,
      "aria-label": `${item.label}: add`,
      "data-key": `${item.key}#text`,
    });
    const add = () => {
      const value = input.value.trim();
      if (value) onAdd(value);
    };
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") add();
    });
    return [input, h("button", { class: "btn", type: "button", text: "Add", on: { click: add } })];
  }

  function control(item, values) {
    const key = item.key;
    const value = values[key];
    const id = `set-${key.replace(/\W/g, "-")}`;
    if (item.type === "switch" || item.type === "choice-switch") {
      const checked = item.type === "switch" ? value === true : value === item.on;
      const input = h("input", {
        type: "checkbox",
        role: "switch",
        class: "switch",
        id,
        "data-key": key,
        checked,
        "aria-describedby": item.help ? `${id}-help` : null,
      });
      input.checked = checked;
      input.addEventListener("change", () => {
        const next = item.type === "switch" ? input.checked : input.checked ? item.on : item.off;
        save({ [key]: next });
      });
      return { id, nodes: [input] };
    }
    if (item.type === "number") {
      const input = h("input", {
        type: "number",
        id,
        "data-key": key,
        min: item.min,
        max: item.max,
        step: 1,
        value,
        inputmode: "numeric",
      });
      input.addEventListener("change", () => {
        const next = Number(input.value);
        if (!Number.isInteger(next) || next < item.min || next > item.max) {
          setSaveState(`Not saved: “${item.label}” must be a whole number from ${item.min} to ${item.max}.`, "err");
          input.value = value;
          return;
        }
        if (next !== value) save({ [key]: next });
      });
      return { id, nodes: [input, h("span", { class: "unit", text: item.unit })] };
    }
    if (item.type === "text") {
      const input = h("input", { type: "text", id, "data-key": key, value, spellcheck: "false", autocomplete: "off" });
      input.addEventListener("change", () => {
        const next = input.value.trim();
        if (next !== value) save({ [key]: next });
      });
      return { id, nodes: [input] };
    }
    if (item.type === "subset") {
      const pool = values[item.of] || [];
      const boxes = pool.map((entry) => {
        const box = h("input", { type: "checkbox", "data-key": `${key}#${entry}` });
        box.checked = value.includes(entry);
        box.addEventListener("change", () => {
          const next = pool.filter((p) => (p === entry ? box.checked : value.includes(p)));
          save({ [key]: next });
        });
        return h("label", null, box, appName(entry));
      });
      return { id: null, stacked: true, nodes: [h("div", { class: "checks" }, boxes)] };
    }
    // apps and list: chips, then a way to add.
    const remove = (entry) => {
      const next = value.filter((v) => v !== entry);
      const changes = { [key]: next };
      // An app that stops being an away app can't stay marked as hard to leave.
      if (key === "away.apps") changes["away.immersive"] = values["away.immersive"].filter((v) => next.includes(v));
      save(changes);
    };
    const add = (entry) => {
      if (!value.includes(entry)) save({ [key]: [...value, entry] });
    };
    const nodes = [chips(item, value, remove)];
    const adder = h("div", { class: "adder" });
    if (item.type === "apps") {
      const candidates = (config.running_apps || []).filter((a) => !a.home && !value.includes(a.id));
      if (candidates.length) {
        const select = h(
          "select",
          { "aria-label": `${item.label}: pick a running app`, "data-key": `${key}#pick` },
          h("option", { value: "", text: "Pick a running app…" }),
          candidates.map((a) => h("option", { value: a.id, text: a.name }))
        );
        select.addEventListener("change", () => {
          if (select.value) add(select.value);
        });
        adder.append(select, h("span", { class: "unit", text: "or" }));
      }
      adder.append(...textAdder(item, "its id, e.g. com.apple.Notes", add));
    } else {
      adder.append(...textAdder(item, item.placeholder || "", add));
    }
    nodes.push(adder);
    return { id: null, stacked: true, nodes };
  }

  function setting(item, values) {
    if (item.showIf && !item.showIf(values)) return null;
    if (item.type === "subset" && !(values[item.of] || []).length) return null;
    const built = control(item, values);
    const restart = (config.restart_keys || []).includes(item.key);
    const name = h(
      built.id ? "label" : "div",
      { class: "name", for: built.id },
      item.label,
      restart ? h("span", { class: "badge", text: "after restart" }) : null
    );
    return h(
      "div",
      { class: `setting${built.stacked ? " stacked" : ""}` },
      name,
      h("div", { class: "control" }, built.nodes),
      item.help ? h("div", { class: "help", id: built.id ? `${built.id}-help` : null, text: item.help }) : null
    );
  }

  const openGroups = new Set();

  function renderConfig() {
    if (!config) return;
    const root = $("config-root");
    root.setAttribute("aria-busy", "false");
    const values = config.values;
    const cards = GROUPS.map((group) => {
      const body = [
        group.intro ? h("p", { class: "group-intro", text: group.intro }) : null,
        group.items.map((item) => setting(item, values)),
      ];
      if (group.collapsed) {
        const details = h("details", { class: "card" }, h("summary", { text: group.title }), body);
        details.open = openGroups.has(group.title);
        details.addEventListener("toggle", () => {
          if (details.open) openGroups.add(group.title);
          else openGroups.delete(group.title);
        });
        return details;
      }
      return h("section", { class: "card" }, h("h2", { text: group.title }), body);
    });
    const where = h(
      "p",
      { class: "foot" },
      h(
        "span",
        null,
        "Changes are saved to ",
        h("code", { text: config.config_path }),
        " as you make them and apply at once, except hotkeys. The port (",
        String(config.port),
        ") and the access token are managed by focusctl install-hooks."
      )
    );
    root.replaceChildren(...cards, where);
  }

  async function loadConfig() {
    try {
      config = await api("/api/config");
      showNotice("");
      renderConfig();
    } catch (error) {
      explain(error);
    }
  }

  // ---------- tabs ----------

  const tabs = [
    { tab: $("tab-stats"), panel: $("panel-stats"), show: () => loadStats(true) },
    { tab: $("tab-config"), panel: $("panel-config"), show: loadConfig },
  ];

  function select(index, focus) {
    tabs.forEach((t, i) => {
      const on = i === index;
      t.tab.setAttribute("aria-selected", String(on));
      t.tab.tabIndex = on ? 0 : -1;
      t.panel.hidden = !on;
    });
    hideTip();
    store("set", ["focus-autopilot-tab", String(index)]);
    if (focus) tabs[index].tab.focus();
    tabs[index].show();
  }

  tabs.forEach((t, i) => {
    t.tab.addEventListener("click", () => select(i, false));
    t.tab.addEventListener("keydown", (event) => {
      const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
      if (step) select((i + step + tabs.length) % tabs.length, true);
    });
  });

  window.addEventListener("scroll", hideTip, { passive: true });

  let refreshing = false;

  function start() {
    showNotice("");
    select(store("get", "focus-autopilot-tab") === "1" ? 1 : 0, false);
    if (refreshing) return;
    refreshing = true;
    window.setInterval(() => {
      if (!document.hidden && !$("panel-stats").hidden) loadStats(false);
    }, REFRESH_MS);
  }

  // The key can also arrive later, in a page that is already open.
  window.addEventListener("hashchange", () => {
    if (adoptKey()) start();
  });

  if (token) {
    start();
  } else {
    explain(new Refused());
    $("stats-root").setAttribute("aria-busy", "false");
  }
})();
