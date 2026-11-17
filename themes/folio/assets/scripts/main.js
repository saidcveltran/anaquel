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
      if (event.key === "Escape") setOpen(false);
    });
  }

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
