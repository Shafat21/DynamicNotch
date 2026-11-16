// Latest release info (version and download links) from GitHub's public API; the page works without it
// (the links fall back to the Releases page). No cookies, no tracking.
document.documentElement.classList.add("js");

const REPO = "Shafat21/DynamicNotch";
const pick = (assets, re) => assets.find((a) => re.test(a.name))?.browser_download_url;

fetch(`https://api.github.com/repos/${REPO}/releases/latest`)
  .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
  .then((rel) => {
    const ver = String(rel.tag_name || "").replace(/^v/, "");
    if (ver) document.querySelectorAll(".js-ver").forEach((e) => (e.textContent = ver));
    const set = (sel, url) => url && document.querySelectorAll(sel).forEach((a) => (a.href = url));
    set(".js-win", pick(rel.assets, /setup\.exe$/i));
    set(".js-deb", pick(rel.assets, /\.deb$/i));
    set(".js-rpm", pick(rel.assets, /\.rpm$/i));
    set(".js-appimage", pick(rel.assets, /\.AppImage$/i));
  })
  .catch(() => {});

// reveal sections as they scroll into view
const io = "IntersectionObserver" in window && new IntersectionObserver((entries) => {
  for (const e of entries) if (e.isIntersecting) (e.target.classList.add("in"), io.unobserve(e.target));
}, { threshold: 0.12 });
document.querySelectorAll(".reveal").forEach((el) => (io ? io.observe(el) : el.classList.add("in")));
