// Runs before first paint so the saved theme never flashes. Kept tiny on purpose.
try {
  var saved = localStorage.getItem("anaquel-theme");
  if (saved === "light" || saved === "dark") {
    document.documentElement.setAttribute("data-theme", saved);
  }
} catch (error) {
  // Storage can be blocked; the system preference still applies.
}
