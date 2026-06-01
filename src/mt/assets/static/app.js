(function () {
  // Sidebar drawers — nav (left) + toc (right). Default open; persisted in localStorage.
  var htmlEl = document.documentElement;
  function readBool(key, fallback) {
    try {
      var v = localStorage.getItem(key);
      if (v === null) return fallback;
      return v === '1';
    } catch (e) { return fallback; }
  }
  function writeBool(key, val) {
    try { localStorage.setItem(key, val ? '1' : '0'); } catch (e) {}
  }
  function applyDrawer(kind, open) {
    htmlEl.dataset[kind + 'Open'] = open ? 'true' : 'false';
    var el = document.querySelector(kind === 'nav' ? '.mt-nav' : '.mt-toc');
    if (el) el.setAttribute('aria-hidden', open ? 'false' : 'true');
  }
  var navBtn = document.querySelector('[data-mt-nav-toggle]');
  var tocBtn = document.querySelector('[data-mt-toc-toggle]');
  var hasNav = !!document.querySelector('.mt-nav');
  var hasToc = !!document.querySelector('.mt-toc');
  if (hasNav) applyDrawer('nav', readBool('mt-nav-open', true));
  if (hasToc) applyDrawer('toc', readBool('mt-toc-open', true));
  if (navBtn) navBtn.addEventListener('click', function () {
    var next = htmlEl.dataset.navOpen !== 'true';
    applyDrawer('nav', next); writeBool('mt-nav-open', next);
  });
  if (tocBtn) tocBtn.addEventListener('click', function () {
    var next = htmlEl.dataset.tocOpen !== 'true';
    applyDrawer('toc', next); writeBool('mt-toc-open', next);
  });
  // Esc closes whichever drawer is open (one per press, toc first then nav).
  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    if (htmlEl.dataset.tocOpen === 'true') {
      applyDrawer('toc', false); writeBool('mt-toc-open', false); return;
    }
    if (htmlEl.dataset.navOpen === 'true') {
      applyDrawer('nav', false); writeBool('mt-nav-open', false);
    }
  });

  // Theme toggle — cycle auto → light → dark → auto.
  // Default is 'auto' (follow OS); explicit choice persists in localStorage.
  var btn = document.querySelector('[data-mt-theme-toggle]');
  var html = document.documentElement;
  function setTheme(t) {
    html.dataset.theme = t;
    try {
      if (t === 'auto') localStorage.removeItem('mt-theme');
      else localStorage.setItem('mt-theme', t);
    } catch (e) {}
  }
  try {
    var saved = localStorage.getItem('mt-theme');
    if (saved === 'light' || saved === 'dark') setTheme(saved);
  } catch (e) {}
  if (btn) {
    btn.addEventListener('click', function () {
      var cur = html.dataset.theme;
      var next = cur === 'auto' ? 'light'
               : cur === 'light' ? 'dark'
               : 'auto';
      setTheme(next);
    });
  }

  // Copy button on <pre> (skip mermaid source blocks — they're consumed by mermaid.js)
  document.querySelectorAll('.mt-content pre:not(.mermaid)').forEach(function (pre) {
    var b = document.createElement('button');
    b.className = 'mt-copy-btn';
    b.type = 'button';
    b.textContent = 'Copy';
    b.addEventListener('click', function () {
      var code = pre.querySelector('code');
      var text = code ? code.innerText : pre.innerText;
      navigator.clipboard.writeText(text).then(function () {
        b.textContent = 'Copied';
        setTimeout(function () { b.textContent = 'Copy'; }, 1200);
      });
    });
    pre.appendChild(b);
  });

  // TOC scrollspy — position-based: pick the last heading whose top is above
  // the activation line (~30% from viewport top). Robust on both up/down scroll.
  var tocLinks = document.querySelectorAll('.mt-toc a[href^="#"]');
  if (tocLinks.length) {
    var linkById = {};
    tocLinks.forEach(function (a) {
      var id = decodeURIComponent(a.getAttribute('href').slice(1));
      linkById[id] = a;
    });
    var headings = Object.keys(linkById)
      .map(function (id) { return document.getElementById(id); })
      .filter(Boolean);
    if (headings.length) {
      var current = null;
      function setActive(link) {
        if (link === current) return;
        if (current) current.classList.remove('is-active');
        current = link;
        if (current) {
          current.classList.add('is-active');
          // Keep active link visible inside the sidebar.
          var sidebar = current.closest('.mt-toc');
          if (sidebar) {
            var lr = current.getBoundingClientRect();
            var sr = sidebar.getBoundingClientRect();
            if (lr.top < sr.top + 8 || lr.bottom > sr.bottom - 8) {
              current.scrollIntoView({ block: 'nearest' });
            }
          }
        }
      }
      function update() {
        var line = window.innerHeight * 0.3;
        var found = null;
        for (var i = 0; i < headings.length; i++) {
          var top = headings[i].getBoundingClientRect().top;
          if (top - line <= 0) {
            found = headings[i];
          } else {
            break;
          }
        }
        // Edge cases:
        // - near top of doc → highlight the first heading
        // - bottom of doc (within ~2px of max scroll) → highlight the last
        if (!found && headings.length) found = headings[0];
        if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4) {
          found = headings[headings.length - 1];
        }
        if (found) setActive(linkById[found.id]);
      }
      var ticking = false;
      function onScroll() {
        if (ticking) return;
        ticking = true;
        requestAnimationFrame(function () { update(); ticking = false; });
      }
      window.addEventListener('scroll', onScroll, { passive: true });
      window.addEventListener('resize', onScroll, { passive: true });
      // Click handler — mark active immediately so the UI doesn't lag while
      // the browser scrolls to the anchor.
      tocLinks.forEach(function (a) {
        a.addEventListener('click', function () { setActive(a); });
      });
      update();
    }
  }
})();
