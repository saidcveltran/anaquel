"use strict";

// The interface of `anaquel studio`: a page list, a text box, a preview and a save button.
// No framework and no build step. Text from the library is only ever put into the page with
// `textContent` or into the sandboxed preview frame, never as markup of this page.
(() => {
  const API = "/__studio/api/";
  const PREVIEW_DELAY_MS = 400;
  const LANGUAGES = ["en", "es"];

  const $ = (id) => document.getElementById(id);
  const el = {
    language: $("language"),
    notice: $("notice"),
    filter: $("filter"),
    pageList: $("page-list"),
    problems: $("problems"),
    problemList: $("problem-list"),
    editorEmpty: $("editor-empty"),
    editorBody: $("editor-body"),
    currentPath: $("current-path"),
    directive: $("directive"),
    save: $("save"),
    state: $("state"),
    text: $("text"),
    pageDiagnostics: $("page-diagnostics"),
    previewNote: $("preview-note"),
    frame: $("frame"),
    elsewhere: $("elsewhere"),
    elsewhereList: $("elsewhere-list"),
    choice: $("choice"),
    choiceMessage: $("choice-message"),
    choiceButtons: $("choice-buttons"),
  };

  const app = {
    language: "en",
    texts: {},
    fallback: {},
    pages: [],
    // The page open in the editor: its path, the revision it was loaded or saved at, and the
    // text that was on disk then.
    current: null,
    previewTicket: 0,
    previewTimer: null,
    lastHtml: null,
    saving: false,
  };

  // ----- texts ---------------------------------------------------------------------------

  function t(key, values) {
    let text = app.texts[key];
    if (text === undefined) text = app.fallback[key];
    if (text === undefined) return key;
    for (const [name, value] of Object.entries(values || {})) {
      text = text.split("{" + name + "}").join(value);
    }
    return text;
  }

  async function fetchTexts(language) {
    const response = await fetch("/__studio/texts." + language + ".json", { credentials: "same-origin" });
    if (!response.ok) throw new Error("texts " + language + ": " + response.status);
    return response.json();
  }

  function applyTexts() {
    document.documentElement.lang = app.language;
    document.title = t("app.title");
    for (const node of document.querySelectorAll("[data-text]")) {
      node.textContent = t(node.dataset.text);
    }
    for (const node of document.querySelectorAll("[data-attribute]")) {
      const [attribute, key] = node.dataset.attribute.split(":");
      node.setAttribute(attribute, t(key));
    }
    buildDirectiveMenu();
    renderPages();
    renderState();
  }

  function chooseLanguage() {
    let saved = null;
    try {
      saved = window.localStorage.getItem("anaquel-studio-language");
    } catch (error) {
      saved = null;
    }
    if (LANGUAGES.includes(saved)) return saved;
    const preferred = (navigator.language || "en").slice(0, 2).toLowerCase();
    return LANGUAGES.includes(preferred) ? preferred : "en";
  }

  async function setLanguage(language) {
    // English is always loaded: a key missing from another language falls back to it.
    if (Object.keys(app.fallback).length === 0) app.fallback = await fetchTexts("en");
    app.texts = language === "en" ? app.fallback : await fetchTexts(language);
    app.language = language;
    el.language.value = language;
    try {
      window.localStorage.setItem("anaquel-studio-language", language);
    } catch (error) {
      // The choice is only a convenience; the page works without storing it.
    }
    applyTexts();
  }

  // ----- requests ------------------------------------------------------------------------

  async function request(method, path, body) {
    const options = { method, credentials: "same-origin", headers: {} };
    if (body !== undefined) {
      options.headers["Content-Type"] = "application/json";
      options.body = JSON.stringify(body);
    }
    let response;
    try {
      response = await fetch(API + path, options);
    } catch (error) {
      showNotice(t("connection.lost"));
      throw error;
    }
    let data = null;
    try {
      data = await response.json();
    } catch (error) {
      data = null;
    }
    return { ok: response.ok, status: response.status, data };
  }

  function errorText(data) {
    const error = data && data.error;
    if (!error) return t("connection.lost");
    const key = "error." + error.code;
    let text = app.texts[key] !== undefined || app.fallback[key] !== undefined ? t(key) : error.message;
    if (Array.isArray(error.unrestored) && error.unrestored.length > 0) {
      text += " " + error.unrestored.join(", ");
    }
    return text;
  }

  function showNotice(message) {
    el.notice.textContent = message;
    el.notice.hidden = false;
  }

  function clearNotice() {
    el.notice.hidden = true;
    el.notice.textContent = "";
  }

  // ----- a question with buttons, in the page ----------------------------------------------

  function choose(messageKey, options) {
    return new Promise((resolve) => {
      el.choiceMessage.textContent = t(messageKey);
      el.choiceButtons.replaceChildren();
      for (const option of options) {
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = t(option.label);
        button.addEventListener("click", () => {
          el.choice.hidden = true;
          resolve(option.id);
        });
        el.choiceButtons.append(button);
      }
      el.choice.hidden = false;
      el.choiceButtons.firstChild.focus();
    });
  }

  // ----- the list of pages -------------------------------------------------------------------

  async function loadPages() {
    const { ok, data } = await request("GET", "pages");
    if (!ok) {
      showNotice(errorText(data));
      return;
    }
    clearNotice();
    app.pages = data.pages;
    el.problemList.replaceChildren(
      ...data.problems.map((problem) => {
        const item = document.createElement("li");
        item.textContent = problem;
        return item;
      }),
    );
    el.problems.hidden = data.problems.length === 0;
    renderPages();
  }

  function groupLabel(page) {
    return page.shelf ? page.shelf + " / " + page.volume : page.volume;
  }

  function fileName(path) {
    return path.slice(path.lastIndexOf("/") + 1);
  }

  function renderPages() {
    const needle = el.filter.value.trim().toLowerCase();
    const shown = app.pages.filter((page) =>
      (page.path + " " + (page.title || "")).toLowerCase().includes(needle),
    );
    const nodes = [];
    let group = null;
    for (const page of shown) {
      if (groupLabel(page) !== group) {
        group = groupLabel(page);
        const heading = document.createElement("div");
        heading.className = "group";
        heading.textContent = group;
        nodes.push(heading);
      }
      nodes.push(pageButton(page));
    }
    if (nodes.length === 0) {
      const none = document.createElement("p");
      none.className = "empty";
      none.textContent = t("pages.none");
      nodes.push(none);
    }
    el.pageList.replaceChildren(...nodes);
  }

  function pageButton(page) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "page";
    button.textContent = page.title || fileName(page.path);
    button.title = page.path;
    if (app.current && app.current.path === page.path) button.setAttribute("aria-current", "true");
    const kind = page.status.kind;
    const badgeKey = { draft: "pages.draft", invalid: "pages.invalid", unavailable: "pages.unavailable" }[kind];
    if (badgeKey) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = t(badgeKey);
      button.append(badge);
    }
    if (kind === "unavailable") {
      button.classList.add("unavailable");
      button.disabled = true;
      button.title = page.path + " — " + page.status.reason;
    } else {
      button.addEventListener("click", () => openPage(page.path));
    }
    return button;
  }

  // ----- the editor ---------------------------------------------------------------------------

  function isDirty() {
    return app.current !== null && el.text.value !== app.current.saved;
  }

  function renderState(kind) {
    let key = "state.clean";
    let className = "state";
    if (kind === "saving") {
      key = "state.saving";
    } else if (kind === "saved" && !isDirty()) {
      key = "state.saved";
      className += " saved";
    } else if (kind === "failed") {
      key = "state.failed";
      className += " failed";
    } else if (isDirty()) {
      key = "state.dirty";
      className += " dirty";
    }
    el.state.textContent = t(key);
    el.state.className = className;
  }

  async function openPage(path) {
    if (app.current && app.current.path === path) return;
    if (isDirty()) {
      const answer = await choose("switch.message", [
        { id: "save", label: "switch.save" },
        { id: "discard", label: "switch.discard" },
        { id: "cancel", label: "switch.cancel" },
      ]);
      if (answer === "cancel") return;
      if (answer === "save" && !(await save())) return;
    }
    await loadPage(path);
  }

  async function loadPage(path) {
    const { ok, data } = await request("GET", "page?path=" + encodeURIComponent(path));
    if (!ok) {
      showNotice(errorText(data));
      return;
    }
    clearNotice();
    app.current = { path: data.path, revision: data.revision, saved: data.text };
    app.lastHtml = null;
    el.text.value = data.text;
    el.currentPath.textContent = data.path;
    el.editorEmpty.hidden = true;
    el.editorBody.hidden = false;
    renderPages();
    renderState();
    runPreview();
  }

  async function save() {
    if (!app.current || app.saving) return false;
    app.saving = true;
    renderState("saving");
    const sent = el.text.value;
    try {
      const { ok, status, data } = await request("PUT", "page", {
        path: app.current.path,
        text: sent,
        revision: app.current.revision,
      });
      if (ok) {
        app.current.revision = data.revision;
        app.current.saved = sent;
        clearNotice();
        renderState("saved");
        loadPages();
        return true;
      }
      renderState("failed");
      if (status === 409) {
        await resolveConflict();
      } else {
        showNotice(errorText(data));
      }
      return false;
    } catch (error) {
      renderState("failed");
      return false;
    } finally {
      app.saving = false;
    }
  }

  async function resolveConflict() {
    const answer = await choose("conflict.message", [
      { id: "reload", label: "conflict.reload" },
      { id: "keep", label: "conflict.keep" },
    ]);
    if (answer === "reload") {
      const path = app.current.path;
      app.current = null;
      await loadPage(path);
    }
  }

  // ----- the preview ------------------------------------------------------------------------------

  function schedulePreview() {
    window.clearTimeout(app.previewTimer);
    app.previewTimer = window.setTimeout(runPreview, PREVIEW_DELAY_MS);
  }

  async function runPreview() {
    if (!app.current) return;
    const ticket = ++app.previewTicket;
    let result;
    try {
      result = await request("POST", "preview", { path: app.current.path, text: el.text.value });
    } catch (error) {
      return;
    }
    if (ticket !== app.previewTicket) return;
    if (!result.ok) {
      showNotice(errorText(result.data));
      return;
    }
    clearNotice();
    showPreview(result.data);
  }

  function showPreview(preview) {
    renderDiagnostics(el.pageDiagnostics, preview.page);
    renderDiagnostics(el.elsewhereList, preview.elsewhere);
    el.elsewhere.hidden = preview.elsewhere.length === 0;
    const notes = [];
    if (preview.html !== null) {
      app.lastHtml = preview.html;
    } else {
      notes.push(app.lastHtml !== null ? t("preview.outdated") : t("preview.none"));
    }
    if (preview.draft) notes.push(t("preview.draft"));
    if (preview.frame_origins.length > 0) {
      notes.push(t("preview.frames", { origins: preview.frame_origins.join(", ") }));
    }
    el.previewNote.textContent = notes.join(" ");
    el.previewNote.hidden = notes.length === 0;
    el.frame.srcdoc = app.lastHtml !== null ? app.lastHtml : "";
  }

  function renderDiagnostics(list, items) {
    list.replaceChildren(
      ...items.map((item) => {
        const row = document.createElement("li");
        row.className = item.severity;
        const code = document.createElement("span");
        code.className = "code";
        code.textContent = item.code;
        row.append(code);
        const where = item.line === null ? "" : "(" + item.line + ") ";
        row.append(document.createTextNode(where + item.message));
        if (item.help) {
          const help = document.createElement("span");
          help.className = "help";
          help.textContent = item.help;
          row.append(help);
        }
        return row;
      }),
    );
  }

  // The frame has no scripts; the links inside it must not take the preview away from the page.
  function keepFrameInPlace() {
    const doc = el.frame.contentDocument;
    if (!doc) return;
    doc.addEventListener("click", (event) => {
      if (event.target instanceof Element && event.target.closest("a")) event.preventDefault();
    });
  }

  // ----- directives ----------------------------------------------------------------------------

  function directives() {
    const text = t("snippet.text");
    const one = t("snippet.one");
    const two = t("snippet.two");
    const pair = (outer, inner, attributes) =>
      `::::${outer}\n` +
      [one, two].map((title) => `:::${inner}${attributes(title)}\n${text}\n:::\n`).join("") +
      "::::\n";
    const titled = (title) => ` title="${title}"`;
    return [
      { id: "hint", label: "directive.hint", build: () => `:::hint\n${text}\n:::\n` },
      { id: "warning", label: "directive.warning", build: () => `:::hint warning\n${text}\n:::\n` },
      { id: "danger", label: "directive.danger", build: () => `:::hint danger\n${text}\n:::\n` },
      {
        id: "details",
        label: "directive.details",
        build: () => `:::details title="${t("snippet.title")}"\n${text}\n:::\n`,
      },
      { id: "tabs", label: "directive.tabs", build: () => pair("tabs", "tab", titled) },
      { id: "cards", label: "directive.cards", build: () => pair("cards cols=2", "card", titled) },
      { id: "steps", label: "directive.steps", build: () => pair("steps", "step", titled) },
      { id: "columns", label: "directive.columns", build: () => pair("columns", "column", () => "") },
      {
        id: "embed",
        label: "directive.embed",
        build: () => `:::embed url="${t("snippet.address")}"\n:::\n`,
      },
    ];
  }

  function buildDirectiveMenu() {
    const placeholder = el.directive.options[0];
    el.directive.replaceChildren(placeholder);
    for (const directive of directives()) {
      const option = document.createElement("option");
      option.value = directive.id;
      option.textContent = t(directive.label);
      el.directive.append(option);
    }
  }

  // A directive stands alone in its block, so it is separated from the text around it by blank lines.
  function insertDirective(id) {
    const directive = directives().find((candidate) => candidate.id === id);
    if (!directive || !app.current) return;
    const { selectionStart: start, selectionEnd: end, value } = el.text;
    const before = value.slice(0, start);
    let lead = "";
    if (before.length > 0) lead = before.endsWith("\n\n") ? "" : before.endsWith("\n") ? "\n" : "\n\n";
    const after = value.slice(end);
    // The snippet ends with a line end; text that follows on the next line needs one more.
    const trail = after.length === 0 || after.startsWith("\n") ? "" : "\n";
    el.text.focus();
    el.text.setRangeText(lead + directive.build() + trail, start, end, "end");
    onTextChanged();
  }

  // ----- events ---------------------------------------------------------------------------------

  function onTextChanged() {
    renderState();
    schedulePreview();
  }

  el.text.addEventListener("input", onTextChanged);
  el.save.addEventListener("click", save);
  el.filter.addEventListener("input", renderPages);
  el.frame.addEventListener("load", keepFrameInPlace);
  el.language.addEventListener("change", () => setLanguage(el.language.value));
  el.directive.addEventListener("change", () => {
    const id = el.directive.value;
    el.directive.value = "";
    if (id) insertDirective(id);
  });
  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      save();
    }
  });
  window.addEventListener("beforeunload", (event) => {
    if (isDirty()) {
      event.preventDefault();
      event.returnValue = "";
    }
  });

  setLanguage(chooseLanguage())
    .then(loadPages)
    .catch(() => showNotice(t("connection.lost")));
})();
