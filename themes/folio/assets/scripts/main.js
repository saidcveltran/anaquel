// Progressive enhancement only: every page reads fine without this file.
(function () {
  "use strict";
  var root = document.documentElement;

  function currentTheme() {
    var set = root.getAttribute("data-theme");
    if (set) return set;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  var toggle = document.querySelector(".theme-toggle");
  if (toggle) {
    toggle.addEventListener("click", function () {
      var next = currentTheme() === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try {
        localStorage.setItem("anaquel-theme", next);
      } catch (error) {
        // Not saved; it still applies to this page.
      }
    });
  }

  var navToggle = document.querySelector(".nav-toggle");
  if (navToggle) {
    var setOpen = function (open) {
      document.body.classList.toggle("nav-open", open);
      navToggle.setAttribute("aria-expanded", open ? "true" : "false");
    };
    navToggle.addEventListener("click", function () {
      setOpen(!document.body.classList.contains("nav-open"));
    });
    document.addEventListener("keydown", function (event) {
      if (event.key !== "Escape" || !document.body.classList.contains("nav-open")) return;
      setOpen(false);
      navToggle.focus();
    });
  }

  // Tabs: without this script every tab shows, under its title. With it, a tab list appears.
  // Tabs that share a `data-sync` name follow each other and the choice is remembered.
  var syncGroups = {};
  var tabCount = 0;

  function storedTab(sync) {
    try {
      return localStorage.getItem("anaquel-tab-" + sync);
    } catch (error) {
      return null;
    }
  }

  function setupTabs(group) {
    var panels = Array.prototype.filter.call(group.children, function (el) {
      return el.classList.contains("tab");
    });
    if (!panels.length) return;
    var sync = group.getAttribute("data-sync");
    var list = document.createElement("div");
    list.className = "tab-list";
    list.setAttribute("role", "tablist");
    var buttons = panels.map(function (panel) {
      tabCount += 1;
      var id = "tabs-" + tabCount;
      var button = document.createElement("button");
      button.type = "button";
      button.className = "tab-button";
      button.id = id + "-tab";
      button.setAttribute("role", "tab");
      button.setAttribute("aria-controls", id + "-panel");
      button.textContent = panel.getAttribute("data-title") || "";
      panel.id = id + "-panel";
      panel.setAttribute("role", "tabpanel");
      panel.setAttribute("tabindex", "0");
      panel.setAttribute("aria-labelledby", button.id);
      list.appendChild(button);
      return button;
    });

    function show(index, focus) {
      buttons.forEach(function (button, i) {
        var on = i === index;
        button.setAttribute("aria-selected", on ? "true" : "false");
        button.tabIndex = on ? 0 : -1;
        panels[i].hidden = !on;
      });
      if (focus) buttons[index].focus();
    }

    function showTitle(title) {
      var index = buttons.findIndex(function (button) {
        return button.textContent === title;
      });
      if (index < 0) return false;
      show(index, false);
      return true;
    }

    function choose(index) {
      show(index, true);
      if (!sync) return;
      var title = buttons[index].textContent;
      try {
        localStorage.setItem("anaquel-tab-" + sync, title);
      } catch (error) {
        // Not saved; the other tabs still follow on this page.
      }
      syncGroups[sync].forEach(function (other) {
        if (other !== showTitle) other(title);
      });
    }

    buttons.forEach(function (button, i) {
      button.addEventListener("click", function () {
        choose(i);
      });
      button.addEventListener("keydown", function (event) {
        var next = null;
        if (event.key === "ArrowRight") next = (i + 1) % buttons.length;
        else if (event.key === "ArrowLeft") next = (i + buttons.length - 1) % buttons.length;
        else if (event.key === "Home") next = 0;
        else if (event.key === "End") next = buttons.length - 1;
        if (next === null) return;
        event.preventDefault();
        choose(next);
      });
    });

    group.insertBefore(list, group.firstChild);
    group.classList.add("is-ready");
    show(0, false);
    if (sync) {
      (syncGroups[sync] = syncGroups[sync] || []).push(showTitle);
      var saved = storedTab(sync);
      if (saved) showTitle(saved);
    }
  }

  Array.prototype.forEach.call(document.querySelectorAll(".tabs"), setupTabs);

  var motionQuery = window.matchMedia("(prefers-reduced-motion: no-preference)");
  var scrollKeys = ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "];
  var running = null;
  root.style.scrollBehavior = "auto";

  function easeOut(t) {
    return 1 - Math.pow(1 - t, 3);
  }

  function stopScroll() {
    if (!running) return;
    cancelAnimationFrame(running.frame);
    running = null;
  }

  function samePageAnchor(link) {
    if (!(link instanceof HTMLAnchorElement)) return null;
    if (link.hasAttribute("download") || (link.target && link.target !== "_self")) return null;
    if (link.origin !== location.origin || link.pathname !== location.pathname) return null;
    if (link.search !== location.search || link.hash.length < 2) return null;
    var id;
    try {
      id = decodeURIComponent(link.hash.slice(1));
    } catch (error) {
      return null;
    }
    var target = document.getElementById(id);
    if (!target || !target.getClientRects().length) return null;
    return target;
  }

  function scrollToTarget(target) {
    stopScroll();
    var padding = parseFloat(getComputedStyle(root).scrollPaddingTop) || 0;
    var limit = root.scrollHeight - window.innerHeight;
    var from = window.pageYOffset;
    var to = Math.max(0, Math.min(target.getBoundingClientRect().top + from - padding, limit));
    var distance = to - from;
    if (Math.abs(distance) < 1) return;
    var duration = Math.min(400, 200 + Math.abs(distance) * 0.05);
    var state = { frame: 0 };
    var begin = null;
    running = state;
    function step(now) {
      if (begin === null) begin = now;
      var t = Math.min(1, (now - begin) / duration);
      window.scrollTo(window.pageXOffset, from + distance * easeOut(t));
      if (t < 1) state.frame = requestAnimationFrame(step);
      else stopScroll();
    }
    state.frame = requestAnimationFrame(step);
  }

  function focusTarget(target) {
    if (!target.hasAttribute("tabindex")) {
      target.setAttribute("tabindex", "-1");
      target.addEventListener(
        "blur",
        function () {
          target.removeAttribute("tabindex");
        },
        { once: true },
      );
    }
    target.focus({ preventScroll: true });
  }

  document.addEventListener("click", function (event) {
    if (event.defaultPrevented || event.button !== 0) return;
    if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    if (!motionQuery.matches || root.getAttribute("data-motion") === "off") return;
    var link = event.target.closest ? event.target.closest("a[href]") : null;
    var target = link ? samePageAnchor(link) : null;
    if (!target) return;
    event.preventDefault();
    if (location.hash !== link.hash) {
      history.pushState(null, "", location.pathname + location.search + link.hash);
    }
    scrollToTarget(target);
    focusTarget(target);
  });

  ["wheel", "touchstart", "mousedown"].forEach(function (name) {
    window.addEventListener(name, stopScroll, { passive: true });
  });
  window.addEventListener("keydown", function (event) {
    if (scrollKeys.indexOf(event.key) >= 0) stopScroll();
  });

  // Search: the index is built by `anaquel build`; its UI is loaded on first use.
  var search = document.getElementById("search");
  if (search && search.dataset.bundle) {
    var bundle = search.dataset.bundle;
    var started = false;
    var start = function () {
      if (started) return;
      started = true;
      var css = document.createElement("link");
      css.rel = "stylesheet";
      css.href = bundle + "pagefind-ui.css";
      document.head.appendChild(css);
      var script = document.createElement("script");
      script.src = bundle + "pagefind-ui.js";
      script.onload = function () {
        new window.PagefindUI({ element: "#search", showSubResults: true, showImages: false });
        var input = search.querySelector("input");
        if (input) input.focus();
      };
      document.head.appendChild(script);
    };
    search.addEventListener("pointerenter", start, { once: true });
    search.addEventListener("focusin", start, { once: true });
    var placeholder = document.createElement("button");
    placeholder.type = "button";
    placeholder.className = "icon-button";
    placeholder.setAttribute("aria-label", "Search");
    placeholder.innerHTML =
      '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"><circle cx="11" cy="11" r="6.5" fill="none" stroke="currentColor" stroke-width="2"/><path d="M16 16l4.5 4.5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';
    placeholder.addEventListener("click", function () {
      start();
      placeholder.remove();
    });
    search.appendChild(placeholder);
  }
})();
