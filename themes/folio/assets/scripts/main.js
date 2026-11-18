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

  var shelfStyle = ".i{fill:#0F1B2D;stroke:#0F1B2D}.a{fill:#1F7A63;stroke:#1F7A63}.p{fill:#F3F5F8;stroke:#F3F5F8}@media (prefers-color-scheme:dark){.i{fill:#E8EDF3;stroke:#E8EDF3}.a{fill:#47C2A1;stroke:#47C2A1}.p{fill:#0F1B2D;stroke:#0F1B2D}}.s{fill:none}.f{stroke:none}";
  var shelfMotion = "@keyframes g{from{transform:scaleY(0)}}@keyframes h{from{opacity:0}}@keyframes w{from{transform:scaleX(.1);opacity:0}}@keyframes p{0%{opacity:0;transform:translateY(14px)}60%{opacity:1;transform:translateY(-4px)}100%{transform:none}}@keyframes d{0%{opacity:0;transform:translateY(-90px)}45%{opacity:1;transform:translateY(5px)}68%{transform:translateY(-4px)}100%{transform:none}}@keyframes t{0%,100%{transform:rotate(0)}50%{transform:rotate(-14deg)}}@keyframes e{0%,88%,100%{transform:none}93%{transform:rotate(-7deg)}}@keyframes l{0%,80%,100%{transform:none}90%{transform:translateY(-7px)}}@keyframes r{0%{stroke-dashoffset:1;opacity:0}2%{opacity:1}100%{stroke-dashoffset:0}}.g,.h,.w,.k,.m,.c,.t,.e{transform-box:fill-box}.g{transform-origin:50% 100%;animation:g .55s cubic-bezier(.2,.8,.3,1) both}.h{animation:h .4s ease-out both}.w{transform-origin:50% 50%;animation:w .5s cubic-bezier(.2,.8,.3,1) both}.k{transform-origin:50% 100%;animation:p .55s cubic-bezier(.3,.7,.3,1) both}.m{transform-origin:50% 100%;animation:l 6s ease-in-out infinite}.c{animation:d .85s cubic-bezier(.3,.7,.4,1) 2.05s both}.t{transform-origin:0 0;animation:t 1.7s ease-in-out 3.4s infinite}.e{transform-origin:50% 100%;animation:e 5.5s ease-in-out 4s infinite}.r{stroke-dasharray:1;animation:r .5s ease-out both}@keyframes v{from{transform:scaleX(0)}}.v{transform-box:fill-box;transform-origin:0 50%;animation:v 1.6s steps(42,end) 3.9s both}";
  var shelfBody = '<path class="i f g"  d="M125.4 60.0L131.4 60.0L31.0 308.0L17.0 308.0Z" style="animation-delay:.05s"/><path class="i f g"  d="M188.6 60.0L194.6 60.0L303.0 308.0L289.0 308.0Z" style="animation-delay:.05s"/><rect class="i f h" x="124.4" y="57.5" width="71.2" height="5" rx="2.5"/><g class="w" style="animation-delay:0.4s"><rect class="a f" x="47.6" y="247.2" width="224.8" height="9.6" rx="2"/><path class="a f" opacity=".32" d="M49.6 256.8L270.4 256.8L260.4 267.9L59.6 267.9Z"/></g><g class="w" style="animation-delay:0.55s"><rect class="i f" x="79.6" y="172.1" width="160.8" height="7.8" rx="2"/><path class="i f" opacity=".32" d="M81.6 179.9L238.4 179.9L230.3 188.9L89.7 188.9Z"/></g><g class="w" style="animation-delay:0.7s"><rect class="i f" x="106.5" y="108.9" width="106.9" height="6.3" rx="2"/><path class="i f" opacity=".32" d="M108.5 115.1L211.5 115.1L205.0 122.3L115.0 122.3Z"/></g><g class="k" style="animation-delay:0.95s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M60.6 247.2L75.6 247.2L89.1 205.2L76.5 205.2Z"/></g><g class="k" style="animation-delay:1.0s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M79.6 247.2L90.6 247.2L102.8 201.2L93.7 201.2Z"/></g><g class="k" style="animation-delay:1.05s"><g class="m" style="animation-delay:5.5s"><path class="a f"  d="M94.6 247.2L111.6 247.2L120.5 199.2L106.6 199.2Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M99.5 237.6H110.9"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M106.7 208.8H116.2"/></g></g><g class="k" style="animation-delay:1.1s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M115.6 247.2L127.6 247.2L133.0 203.2L123.1 203.2Z"/></g><g class="k" style="animation-delay:1.15s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M131.6 247.2L145.6 247.2L148.5 195.2L137.2 195.2Z"/></g><g class="k" style="animation-delay:1.2s"><g class="m" style="animation-delay:6.4s"><path class="a f"  d="M149.6 247.2L165.6 247.2L164.4 193.2L151.7 193.2Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M152.5 236.4H162.9"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M153.8 204.0H162.2"/></g></g><g class="k" style="animation-delay:1.25s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M169.6 247.2L181.6 247.2L177.5 197.2L167.8 197.2Z"/></g><g class="k" style="animation-delay:1.3s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M185.6 247.2L203.6 247.2L194.6 193.2L180.3 193.2Z"/></g><g class="k" style="animation-delay:1.35s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M207.6 247.2L220.6 247.2L209.0 197.2L198.5 197.2Z"/></g><g class="k" style="animation-delay:1.4s"><g class="m" style="animation-delay:7.3s"><path class="a f"  d="M224.6 247.2L239.6 247.2L226.8 205.2L214.3 205.2Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M225.0 238.8H234.5"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M218.8 213.6H226.9"/></g></g><g class="k" style="animation-delay:1.45s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M243.6 247.2L254.6 247.2L238.0 201.2L228.9 201.2Z"/></g><g class="k" style="animation-delay:1.3s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M91.0 172.1L103.0 172.1L114.0 136.1L104.3 136.1Z"/></g><g class="k" style="animation-delay:1.35s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M107.0 172.1L121.0 172.1L130.0 129.1L119.2 129.1Z"/></g><g class="k" style="animation-delay:1.4s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M125.0 172.1L141.0 172.1L145.5 128.1L133.2 128.1Z"/></g><g class="k" style="animation-delay:1.45s"><g class="m" style="animation-delay:8.2s"><path class="a f"  d="M145.0 172.1L157.0 172.1L157.7 131.1L148.3 131.1Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M148.2 163.9H154.6"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M150.1 139.3H155.0"/></g></g><g class="k" style="animation-delay:1.5s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M161.0 172.1L179.0 172.1L174.5 128.1L160.8 128.1Z"/></g><g class="k" style="animation-delay:1.55s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M183.0 172.1L196.0 172.1L188.1 131.1L178.0 131.1Z"/></g><g class="k" style="animation-delay:1.6s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M200.0 172.1L215.0 172.1L205.0 138.1L192.7 138.1Z"/></g><g class="k" style="animation-delay:1.65s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M116.6 108.9L127.6 108.9L136.0 76.9L127.8 76.9Z"/></g><g class="k" style="animation-delay:1.7s"><g class="m" style="animation-delay:9.1s"><path class="a f"  d="M131.6 108.9L148.6 108.9L151.7 74.9L139.4 74.9Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M135.7 102.1H146.7"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M140.3 81.7H148.6"/></g></g><g class="k" style="animation-delay:1.75s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M152.6 108.9L164.6 108.9L163.4 77.9L154.5 77.9Z"/></g><g class="k" style="animation-delay:1.8s"><path class="i s" stroke-width="3.6" stroke-linejoin="round" d="M168.6 108.9L182.6 108.9L176.0 72.9L166.1 72.9Z"/></g><g class="k" style="animation-delay:1.85s"><g class="m" style="animation-delay:10.0s"><path class="a f"  d="M186.6 108.9L202.6 108.9L189.5 70.9L178.4 70.9Z"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M187.5 101.3H197.5"/><path class="p s" stroke-width="2.6" stroke-linecap="round" d="M182.6 78.5H189.6"/></g></g><g class="c"><g transform="translate(0 2)"><path class="i f" d="M112 56C108 38 130 26 158 28C186 30 206 40 204 56Z"/><path class="i s t" stroke-width="8" stroke-linecap="round" d="M200 50C217 50 225 66 218 86"/><circle class="p s" stroke-width="6" cx="124" cy="42" r="15.5"/><circle class="i f" cx="124" cy="42" r="14"/><path class="i f e" d="M111 37L112 19L125 30ZM123 30L136 19L137 37Z"/><path class="p s" stroke-width="2.4" stroke-linecap="round" d="M114.8 41q2.6 2.8 5.2 0M128 41q2.6 2.8 5.2 0"/><path class="p f" d="M122.6 46.8h2.8L124 48.4Z"/></g></g><g class="i s" stroke-width="8" stroke-linecap="round" stroke-linejoin="round" transform="translate(200 72) scale(2.2)"><path class="r" pathLength="1" style="animation-delay:2.1s" d="M58.0 20V60M58.0 40A20 20 0 0 1 98.0 40V60"/><path class="r" pathLength="1" style="animation-delay:2.4s" d="M116 40a20 20 0 1 0 40 0a20 20 0 1 0 -40 0"/><path class="r" pathLength="1" style="animation-delay:2.55s" d="M156.0 20V60"/><path class="r" pathLength="1" style="animation-delay:2.7s" d="M174 40a20 20 0 1 0 40 0a20 20 0 1 0 -40 0"/><path class="r" pathLength="1" style="animation-delay:2.85s" d="M214.0 20V80"/><path class="r" pathLength="1" style="animation-delay:3.0s" d="M232.0 20V40A20 20 0 0 0 272.0 40M272.0 20V60"/><path class="r" pathLength="1" style="animation-delay:3.3s" d="M290.0 40H330.0A20 20 0 1 0 324.14 54.14"/><path class="r" pathLength="1" style="animation-delay:3.6s" d="M348.0 0V60"/></g><defs><clipPath id="u"><rect class="v" x="310" y="266" width="676" height="44"/></clipPath></defs><text class="i f" x="319" y="292" font-size="26" font-family="ui-monospace,Menlo,Consolas,monospace" textLength="655" lengthAdjust="spacing" clip-path="url(#u)">Hello from Team! This is Anaquel Dev Team.</text>';

  function shelfImage(animated) {
    var svg =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 990 320"><style>' +
      shelfStyle +
      (animated ? shelfMotion : "") +
      "</style>" +
      shelfBody +
      "</svg>";
    // DevTools drops a style value with two "url(", and the drawing has a clip-path url inside.
    var data = encodeURIComponent(svg).replace(/\(/g, "%28").replace(/\)/g, "%29");
    return 'url("data:image/svg+xml,' + data + '")';
  }

  if (window.console && typeof console.log === "function") {
    var animated = motionQuery.matches && root.getAttribute("data-motion") !== "off";
    console.log(
      "%c ",
      "font-size:1px;line-height:116px;padding:58px 180px;background:" +
        shelfImage(animated) +
        " no-repeat center/contain",
    );
  }
})();
