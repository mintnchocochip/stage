/* stagecraft engine v1. Inlined into every deck by scripts/build.py. Do not edit per deck. */
(() => {
  'use strict';
  const W = 1920, H = 1080;
  const LEVELS = ['Still', 'Minimal', 'Smooth', 'Cinematic', 'Insane'];
  const qs = new URLSearchParams(location.search);
  const AUDIT = qs.has('audit');
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch {} },
  };
  const G = window.gsap || null;
  if (G) ['SplitText', 'ScrambleTextPlugin'].forEach(n => window[n] && G.registerPlugin(window[n]));

  const stage = document.querySelector('.kx-stage');
  if (!stage) { console.error('[stagecraft] no .kx-stage found'); return; }
  const slides = [...stage.querySelectorAll(':scope > .slide')];
  const sources = (() => {
    try { return JSON.parse(document.getElementById('kx-sources')?.textContent || '{}'); } catch { return {}; }
  })();

  // ---------- motion level ----------
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  // The deck offers only the levels chosen at creation (body data-levels="0 2 4"); all five when unset.
  // Still (0) stays reachable for audit, ?static, PDF and reduced motion even when it is not offered.
  const ALLOWED = (() => {
    const v = [...new Set((document.body.dataset.levels || '').split(/[\s,]+/).filter(Boolean).map(Number))]
      .filter(n => n >= 0 && n <= 4).sort((a, b) => a - b);
    return v.length ? v : [0, 1, 2, 3, 4];
  })();
  const snap = v => ALLOWED.reduce((a, b) => (Math.abs(b - v) < Math.abs(a - v) ? b : a));
  let level = (() => {
    if (AUDIT) return 0;
    if (qs.has('motion')) { const q = clamp(+qs.get('motion') || 0, 0, 4); return q === 0 ? 0 : snap(q); }
    const saved = store.get('kx-motion');
    if (saved !== null && ALLOWED.includes(+saved)) return +saved;
    const def = snap(clamp(+(document.body.dataset.motion ?? 2), 0, 4));
    return reduced ? (ALLOWED.find(n => n <= 1) ?? 0) : def;
  })();
  const effLevel = () => (G ? level : Math.min(level, 1));

  // ---------- stage scaling ----------
  let scale = 1;
  function fit() {
    scale = Math.min(innerWidth / W, innerHeight / H);
    stage.style.transform = `translate(-50%, -50%) scale(${scale})`;
    fxCanvases.forEach(sizeFx);
  }
  // Canvas fx draw in 1920x1080 units; the backing store follows the real pixel density (capped at 4K) so 1440p and 2160p stay sharp.
  const fxCanvases = new Set();
  function sizeFx(f) {
    const k = Math.min(2, Math.max(1, scale * (devicePixelRatio || 1)));
    if (f.k === k) return;
    f.k = k; f.c.width = Math.round(W * k); f.c.height = Math.round(H * k);
    f.ctx.setTransform(k, 0, 0, k, 0, 0);
  }
  addEventListener('resize', fit);
  fit();

  // ---------- pointer (stage coords) ----------
  const ptr = { x: W / 2, y: H / 2, inside: false };
  addEventListener('pointermove', e => {
    const r = stage.getBoundingClientRect();
    ptr.x = (e.clientX - r.left) / scale;
    ptr.y = (e.clientY - r.top) / scale;
    ptr.inside = ptr.x >= 0 && ptr.y >= 0 && ptr.x <= W && ptr.y <= H;
    parallaxPointer();
    hudWake();
  }, { passive: true });

  // ---------- state ----------
  let idx = 0, step = 0, busy = false, enterTl = null;
  const stepsOf = s => [...new Set([...s.querySelectorAll('[data-step]')].map(e => +e.dataset.step))].sort((a, b) => a - b);
  const titleOf = (s, i) => s.dataset.title || (s.querySelector('h1,h2,h3')?.textContent || `Slide ${i + 1}`).trim().replace(/\s+/g, ' ');

  // ---------- animation presets ----------
  const splits = new WeakMap();
  function split(el, type) {
    if (!window.SplitText) return null;
    const key = type;
    const cache = splits.get(el) || {};
    if (!cache[key]) {
      Object.values(cache).forEach(s => s.revert());
      for (const k in cache) delete cache[k];
      cache[key] = window.SplitText.create(el, { type, mask: 'lines', linesClass: 'kx-line' });
      splits.set(el, cache);
    }
    return cache[key];
  }
  const kids = el => [...el.children].filter(c => !c.matches('.notes'));
  const geom = el => (el instanceof SVGGeometryElement ? [el] : [...el.querySelectorAll('path,line,polyline,polygon,circle,rect,ellipse')])
    .filter(g => !g.closest('[data-flow]') && getComputedStyle(g).stroke !== 'none');

  function fmtNum(v, dec) {
    return new Intl.NumberFormat(document.documentElement.lang || 'en', { minimumFractionDigits: dec, maximumFractionDigits: dec }).format(v);
  }

  // Each preset returns a tween/timeline that animates FROM a hidden state TO the element's natural state,
  // so completing it (progress(1)) always leaves the authored layout untouched.
  const P = {
    // Author-designed move: data-move='{"y":140,"rotate":-6,"split":"words","stagger":0.04,"ease":"back.out(1.6)","dur":1.1}'.
    // Distances are authored for Insane and scaled down per level, so one design serves every level.
    move(el, L) {
      let o = {};
      try { o = JSON.parse(el.dataset.move || '{}'); } catch { console.error('[stagecraft] bad data-move JSON', el); }
      if (L < 2) return P.fade(el, L);
      const k = [0, 0, 0.5, 0.8, 1][L];
      const { split: sp, stagger = 0.05, ease = 'expo.out', dur = 1, ...v } = o;
      for (const q of ['x', 'y', 'xPercent', 'yPercent', 'rotate', 'rotateX', 'rotateY', 'skewX', 'skewY']) if (typeof v[q] === 'number') v[q] *= k;
      for (const q of ['scale', 'scaleX', 'scaleY']) if (typeof v[q] === 'number') v[q] = 1 + (v[q] - 1) * k;
      if (v.opacity === undefined) v.opacity = 0;
      if (v.filter) v.clearProps = 'filter';
      let t = el;
      if (sp === 'children') t = kids(el);
      else if (sp) { const r = split(el, sp === 'lines' ? 'lines' : 'lines,' + sp); t = r ? r[sp] : el; }
      return G.from(t, { ...v, duration: dur, ease, stagger });
    },
    fade(el, L) {
      const v = [null,
        { opacity: 0, duration: 0.5, ease: 'power2.out' },
        { opacity: 0, y: 28, duration: 0.8, ease: 'expo.out' },
        { opacity: 0, y: 48, filter: 'blur(10px)', duration: 1.1, ease: 'expo.out', clearProps: 'filter' },
        { opacity: 0, y: 64, scale: 0.96, filter: 'blur(14px)', duration: 1.25, ease: 'expo.out', clearProps: 'filter' }][L];
      return G.from(el, v);
    },
    lines(el, L) {
      if (L < 2) return P.fade(el, L);
      if (L === 4) {
        const s = split(el, 'lines,words'); if (!s) return P.fade(el, L);
        return G.from(s.words, { yPercent: 120, rotate: 6, duration: 1.1, ease: 'expo.out', stagger: 0.025 });
      }
      const s = split(el, 'lines'); if (!s) return P.fade(el, L);
      return L === 2
        ? G.from(s.lines, { yPercent: 105, duration: 0.9, ease: 'expo.out', stagger: 0.08 })
        : G.from(s.lines, { yPercent: 115, rotate: 2.5, transformOrigin: '0% 0%', duration: 1.15, ease: 'expo.out', stagger: 0.1 });
    },
    chars(el, L) {
      if (L < 3) return P.lines(el, L);
      const s = split(el, 'lines,chars'); if (!s) return P.fade(el, L);
      return L === 3
        ? G.from(s.chars, { yPercent: 110, opacity: 0, duration: 0.8, ease: 'expo.out', stagger: 0.018 })
        : G.from(s.chars, { yPercent: 130, rotateX: -90, opacity: 0, transformPerspective: 600, duration: 0.9, ease: 'expo.out', stagger: 0.022 });
    },
    scramble(el, L) {
      if (L < 3 || !window.ScrambleTextPlugin) return P.fade(el, L);
      const text = el.dataset.text || (el.dataset.text = el.textContent);
      el.textContent = ' ';
      return G.to(el, { duration: L === 4 ? 1.6 : 1.1, ease: 'none', scrambleText: { text, chars: el.dataset.chars || 'upperCase', speed: 0.5, revealDelay: 0.15 } });
    },
    stagger(el, L) {
      const c = kids(el); if (!c.length) return P.fade(el, L);
      const v = [null,
        { opacity: 0, duration: 0.45, stagger: 0.04, ease: 'power2.out' },
        { opacity: 0, y: 32, duration: 0.8, stagger: 0.07, ease: 'expo.out' },
        { opacity: 0, y: 40, clipPath: 'inset(0% 0% 100% 0%)', duration: 1.0, stagger: 0.09, ease: 'expo.out', clearProps: 'clipPath' },
        { opacity: 0, y: 40, rotationX: -35, transformOrigin: '50% 0%', transformPerspective: 900, duration: 1.1, stagger: 0.09, ease: 'expo.out' }][L];
      return G.from(c, v);
    },
    mask(el, L) {
      if (L < 2) return P.fade(el, L);
      const from = { left: 'inset(0% 100% 0% 0%)', right: 'inset(0% 0% 0% 100%)', top: 'inset(0% 0% 100% 0%)', bottom: 'inset(100% 0% 0% 0%)' }[el.dataset.from || 'bottom'];
      const tl = G.timeline();
      tl.fromTo(el, { clipPath: from }, { clipPath: 'inset(0% 0% 0% 0%)', duration: L === 4 ? 1.4 : 1.05, ease: L === 4 ? 'power4.inOut' : 'expo.inOut', clearProps: 'clipPath' });
      const inner = el.querySelector('img,video,svg,canvas');
      if (L >= 3 && inner) tl.from(inner, { scale: 1.18, duration: 1.6, ease: 'expo.out' }, 0);
      return tl;
    },
    draw(el, L) {
      if (L < 2) return P.fade(el, L);
      const gs = geom(el); if (!gs.length) return P.fade(el, L);
      const tl = G.timeline();
      gs.forEach((g, i) => {
        let len = 0; try { len = g.getTotalLength(); } catch {}
        if (!len) return;
        tl.fromTo(g, { strokeDasharray: len, strokeDashoffset: len }, {
          strokeDashoffset: 0, duration: L >= 3 ? 1.6 : 1.2, ease: L >= 3 ? 'power2.inOut' : 'power2.out',
          clearProps: 'strokeDasharray,strokeDashoffset',
        }, i * (L >= 3 ? 0.08 : 0.05));
      });
      return tl;
    },
    grow(el, L) {
      if (L < 2) return P.fade(el, L);
      const c = kids(el).length ? kids(el) : [el];
      const x = el.dataset.axis === 'x';
      return G.from(c, {
        [x ? 'scaleX' : 'scaleY']: 0, transformOrigin: x ? '0% 50%' : '50% 100%',
        duration: [0, 0, 0.9, 1.2, 1.3][L], stagger: [0, 0, 0.06, 0.08, 0.07][L],
        ease: L === 4 ? 'elastic.out(1, 0.65)' : 'expo.out',
      });
    },
    pop(el, L) {
      if (L < 2) return P.fade(el, L);
      const c = kids(el).length ? kids(el) : [el];
      return G.from(c, { scale: L === 4 ? 0.2 : 0.5, opacity: 0, duration: 0.7, stagger: 0.06, ease: L === 4 ? 'back.out(2.4)' : 'back.out(1.6)' });
    },
    count(el, L) {
      const nodes = el.matches('[data-count]') ? [el] : [...el.querySelectorAll('[data-count]')];
      const tl = G.timeline();
      nodes.forEach(n => {
        const target = parseFloat(n.dataset.count);
        if (!isFinite(target)) return;
        const dec = n.dataset.decimals !== undefined ? +n.dataset.decimals : ((n.dataset.count.split('.')[1] || '').length);
        const pre = n.dataset.prefix || '', suf = n.dataset.suffix || '';
        const finalText = n.textContent;
        if (L < 2) { tl.add(P.fade(n, L), 0); return; }
        const o = { v: parseFloat(n.dataset.from || 0) };
        n.textContent = pre + fmtNum(o.v, dec) + suf;
        tl.to(o, {
          v: target, duration: [0, 0, 1.5, 2.0, 2.2][L], ease: 'expo.out',
          onUpdate: () => { n.textContent = pre + fmtNum(o.v, dec) + suf; },
          onComplete: () => { n.textContent = finalText; },
        }, 0);
      });
      return tl;
    },
  };

  function animFor(el, L) {
    const type = el.dataset.anim || (el.dataset.move ? 'move' : 'fade');
    if (type === 'none' || L < 1) return null;
    if (L < (+el.dataset.min || 0)) return null;
    const fn = P[type] || P.fade;
    const t = fn(el, L);
    if (t && el.dataset.dur) t.duration(parseFloat(el.dataset.dur));
    return t;
  }

  // ---------- step builds ----------
  function applySteps(s, upto) {
    s.querySelectorAll('[data-step]').forEach(e => e.classList.toggle('kx-hidden', +e.dataset.step > upto));
  }

  function animated(s, filter) {
    return [...s.querySelectorAll('[data-anim],[data-move]')].filter(e => !e.closest('.notes,.kx-kept') && filter(e));
  }
  const stepOf = e => +(e.closest('[data-step]')?.dataset.step || 0);

  // Custom per-slide components: window.KX_ENTER = { 'slide-id': (slide, level, gsap) => timeline | cleanupFn | void }.
  // Level 0 must draw the final static state; that path also feeds ?static screenshots and ?audit.
  const cleanups = new WeakMap();
  function runHook(s, L) {
    const hook = (window.KX_ENTER || {})[s.id];
    if (!hook) return null;
    try {
      const r = hook(s, L, G);
      if (typeof r === 'function') cleanups.set(s, r);
      else if (r && typeof r.progress === 'function') return r;
    } catch (e) { console.error('[stagecraft] KX_ENTER hook failed for #' + s.id, e); }
    return null;
  }
  function stopHook(s) { const c = cleanups.get(s); if (c) { cleanups.delete(s); try { c(); } catch {} } }

  function enter(s, delay = 0) {
    finishEnter();
    const L = effLevel();
    applySteps(s, step);
    stopHook(s);
    const hookTl = runHook(s, L);
    if (L < 1 || !G) { hookTl && hookTl.progress(1); return; }
    const cascade = [0, 0.06, 0.12, 0.15, 0.17][L];
    enterTl = G.timeline({ delay });
    animated(s, e => stepOf(e) === 0).forEach((e, i) => {
      const t = animFor(e, L);
      if (t) enterTl.add(t, e.dataset.delay !== undefined ? parseFloat(e.dataset.delay) : i * cascade);
    });
    if (hookTl) enterTl.add(hookTl, 0);
    watchdog(enterTl);
  }
  // GSAP ticks on requestAnimationFrame. Background tabs, headless capture and throttled windows can stop rAF,
  // which would leave elements frozen at their hidden from-state. A plain timer forces the end state instead.
  function watchdog(tl) {
    if (!tl) return;
    const ms = (tl.duration() + (tl.delay() || 0)) * 1000 + 700;
    setTimeout(() => { if (tl.progress() < 1) tl.progress(1); }, ms);
  }
  function finishEnter() {
    if (enterTl) { enterTl.progress(1).kill(); enterTl = null; }
  }
  function revealStep(s, n) {
    applySteps(s, n);
    const L = effLevel();
    if (L < 1 || !G) return;
    const els = [...s.querySelectorAll(`[data-step="${n}"]`)];
    const tl = G.timeline();
    els.forEach((e, i) => {
      const inner = animated(e, x => stepOf(x) === n);
      if (inner.length) inner.forEach((x, j) => { const t = animFor(x, L); t && tl.add(t, (i + j) * 0.08); });
      else { const t = P.fade(e, L); t && tl.add(t, i * 0.08); }
    });
    watchdog(tl);
  }

  // ---------- parallax ----------
  function parallaxPointer() {
    const L = effLevel();
    if (L < 3 || !G) return;
    const s = slides[idx]; if (!s) return;
    const dx = (ptr.x / W - 0.5), dy = (ptr.y / H - 0.5);
    const k = L === 4 ? 46 : 22;
    s.querySelectorAll('[data-depth]').forEach(l => {
      const d = parseFloat(l.dataset.depth) || 0;
      G.to(l, { x: -dx * k * d * 2, y: -dy * k * d * 2, duration: 1.2, ease: 'power3.out', overwrite: 'auto' });
    });
  }

  // ---------- transitions ----------
  // The deck chooses its cuts (slide data-transition, then body data-cut). The engine never rotates through a set,
  // because a fixed rotation made every deck move the same way. Custom cuts (window.KX_CUT) run from level 2.
  const BUILTIN_CUTS = ['fade', 'slide', 'wipe', 'iris', 'zoom', 'shutter'];
  function pickTransition(to, i, L) {
    const t = to.dataset.transition || document.body.dataset.cut;
    if (t && (window.KX_CUT || {})[t] && L >= 2) return t;
    if (t && BUILTIN_CUTS.includes(t) && (L >= 3 || t === 'fade' || t === 'slide')) return t;
    return L >= 2 ? 'slide' : 'fade';
  }
  const SLIDE_PROPS = 'opacity,transform,clipPath,filter,zIndex,visibility';

  function transition(from, to, dir, i, done) {
    const L = effLevel();
    to.classList.add('is-active');
    if (!G || L < 1 || from === to) {
      from.classList.remove('is-active');
      if (!G && L >= 1) { to.classList.remove('kx-cssfade'); void to.offsetWidth; to.classList.add('kx-cssfade'); }
      done(0); return;
    }
    from.classList.add('is-leaving');
    // Shared elements: [data-keep="id"] on both slides travels from its old box to its new one (magic move).
    const kept = [...to.querySelectorAll('[data-keep]')].map(b => [from.querySelector(`[data-keep="${CSS.escape(b.dataset.keep)}"]`), b]).filter(([a]) => a);
    const type = kept.length && !to.dataset.transition ? 'fade' : pickTransition(to, i, L);
    const layers = to.querySelectorAll('[data-depth]');
    const tl = G.timeline({
      onComplete() {
        kept.forEach(([a, b]) => { G.set(a, { clearProps: 'opacity' }); b.classList.remove('kx-kept'); });
        from.classList.remove('is-active', 'is-leaving');
        G.set([from, to], { clearProps: SLIDE_PROPS });
        if (layers.length) G.set(layers, { clearProps: 'transform' });
        busy = false;
      },
    });
    G.set(to, { zIndex: 2 }); G.set(from, { zIndex: 1 });
    let overlap = 0.3;
    kept.forEach(([a, b]) => {
      const ra = a.getBoundingClientRect(), rb = b.getBoundingClientRect();
      if (!rb.width || !rb.height) return;
      b.classList.add('kx-kept');
      G.set(a, { opacity: 0 });
      tl.from(b, { x: (ra.left - rb.left) / scale, y: (ra.top - rb.top) / scale, scaleX: ra.width / rb.width, scaleY: ra.height / rb.height,
        transformOrigin: '0 0', duration: [0, 0.5, 0.8, 1.0, 1.15][L], ease: 'expo.inOut', clearProps: 'transform' }, 0);
    });
    const custom = (window.KX_CUT || {})[type];
    if (custom) {
      try { const r = custom({ from, to, dir, level: L, gsap: G, tl }); overlap = typeof r === 'number' ? r : 0.4; }
      catch (e) { console.error('[stagecraft] KX_CUT failed: ' + type, e); tl.fromTo(to, { opacity: 0 }, { opacity: 1, duration: 0.45 }); }
    } else switch (type) {
      case 'fade':
        tl.fromTo(to, { opacity: 0 }, { opacity: 1, duration: 0.45, ease: 'power2.out' }).to(from, { opacity: 0, duration: 0.45 }, 0);
        overlap = 0.15; break;
      case 'slide':
        tl.fromTo(to, { opacity: 0, y: dir * 90 }, { opacity: 1, y: 0, duration: 0.85, ease: 'expo.out' })
          .to(from, { opacity: 0, y: -dir * 60, duration: 0.5, ease: 'power2.in' }, 0);
        if (layers.length) tl.from(layers, { y: (k, l) => dir * 260 * (parseFloat(l.dataset.depth) || 0), duration: 1.1, ease: 'expo.out' }, 0);
        overlap = 0.25; break;
      case 'iris': {
        const at = `${(ptr.inside ? ptr.x / W : 0.5) * 100}% ${(ptr.inside ? ptr.y / H : 0.5) * 100}%`;
        tl.fromTo(to, { clipPath: `circle(0% at ${at})` }, { clipPath: `circle(150% at ${at})`, duration: 1.15, ease: 'power3.inOut' })
          .to(from, { scale: 0.94, opacity: 0.4, duration: 1.15, ease: 'power3.inOut' }, 0);
        overlap = 0.5; break;
      }
      case 'zoom':
        tl.fromTo(to, { opacity: 0, scale: 0.86 }, { opacity: 1, scale: 1, duration: 1.1, ease: 'expo.inOut' })
          .to(from, { opacity: 0, scale: 1.18, filter: 'blur(10px)', duration: 1.0, ease: 'expo.inOut' }, 0);
        overlap = 0.55; break;
      case 'shutter': {
        const n = 7, bands = document.createElement('div');
        bands.className = 'kx-shutter';
        for (let k = 0; k < n; k++) bands.appendChild(document.createElement('i'));
        stage.appendChild(bands);
        const b = bands.children;
        G.set(to, { opacity: 0 });
        tl.fromTo(b, { scaleY: 0, transformOrigin: dir > 0 ? '50% 100%' : '50% 0%' }, { scaleY: 1, duration: 0.5, ease: 'power4.in', stagger: 0.04 })
          .add(() => { G.set(to, { opacity: 1 }); G.set(from, { opacity: 0 }); })
          .to(b, { scaleY: 0, transformOrigin: dir > 0 ? '50% 0%' : '50% 100%', duration: 0.6, ease: 'power4.out', stagger: 0.04 })
          .add(() => bands.remove());
        overlap = 0.75; break;
      }
      case 'wipe':
      default:
        tl.fromTo(to, { clipPath: dir > 0 ? 'inset(0% 0% 0% 100%)' : 'inset(0% 100% 0% 0%)' },
          { clipPath: 'inset(0% 0% 0% 0%)', duration: 1.0, ease: 'power4.inOut' })
          .to(from, { x: -dir * 140, scale: 0.96, filter: 'brightness(0.55)', duration: 1.0, ease: 'power4.inOut' }, 0);
        if (layers.length) tl.from(layers, { x: (k, l) => dir * 420 * (parseFloat(l.dataset.depth) || 0), duration: 1.3, ease: 'expo.out' }, 0.1);
        overlap = 0.45;
    }
    busy = true;
    watchdog(tl);
    done(tl.duration() * overlap);
  }

  // ---------- canvas FX ----------
  const fxCache = new WeakMap();
  const cssVar = (el, n, fb) => getComputedStyle(el).getPropertyValue(n).trim() || fb;
  function fxFor(s) {
    const kind = s.dataset.fx;
    if (!kind || !FX[kind]) return null;
    let f = fxCache.get(s);
    if (!f) {
      const c = document.createElement('canvas');
      c.className = 'kx-fx';
      s.prepend(c);
      f = { c, ctx: c.getContext('2d'), raf: 0, run: null, kind, k: 0 };
      sizeFx(f); fxCanvases.add(f);
      fxCache.set(s, f);
    }
    return f;
  }
  function fxStart(s) {
    const L = effLevel();
    const f = fxFor(s); if (!f) return;
    f.c.style.display = L >= 3 ? '' : 'none';
    if (L < 3) return;
    const color = cssVar(s, '--fx', cssVar(s, '--accent', '#ffffff'));
    const step = FX[f.kind](f.ctx, color, L);
    const t0 = performance.now();
    const loop = now => { step((now - t0) / 1000); f.raf = requestAnimationFrame(loop); };
    f.raf = requestAnimationFrame(loop);
  }
  function fxStop(s) {
    const f = fxCache.get(s); if (f) { cancelAnimationFrame(f.raf); f.raf = 0; }
  }
  const rnd = (a, b) => a + Math.random() * (b - a);
  const FX = {
    constellation(ctx, color, L) {
      const n = L === 4 ? 110 : 60, link = 170;
      const pts = Array.from({ length: n }, () => ({ x: rnd(0, W), y: rnd(0, H), vx: rnd(-0.25, 0.25), vy: rnd(-0.25, 0.25) }));
      return () => {
        ctx.clearRect(0, 0, W, H);
        ctx.fillStyle = ctx.strokeStyle = color;
        for (const p of pts) {
          p.x = (p.x + p.vx + W) % W; p.y = (p.y + p.vy + H) % H;
          if (L === 4 && ptr.inside) {
            const dx = ptr.x - p.x, dy = ptr.y - p.y, d = Math.hypot(dx, dy);
            if (d < 260) { p.x -= dx / d * 0.6; p.y -= dy / d * 0.6; }
          }
        }
        for (let a = 0; a < n; a++) {
          const p = pts[a];
          ctx.globalAlpha = 0.55; ctx.beginPath(); ctx.arc(p.x, p.y, 2, 0, 7); ctx.fill();
          for (let b = a + 1; b < n; b++) {
            const q = pts[b], d = Math.hypot(p.x - q.x, p.y - q.y);
            if (d < link) { ctx.globalAlpha = (1 - d / link) * 0.28; ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); ctx.stroke(); }
          }
          if (ptr.inside) {
            const d = Math.hypot(p.x - ptr.x, p.y - ptr.y);
            if (d < 240) { ctx.globalAlpha = (1 - d / 240) * 0.5; ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(ptr.x, ptr.y); ctx.stroke(); }
          }
        }
        ctx.globalAlpha = 1;
      };
    },
    flowfield(ctx, color, L) {
      const n = L === 4 ? 1400 : 600;
      const ps = Array.from({ length: n }, () => ({ x: rnd(0, W), y: rnd(0, H) }));
      ctx.clearRect(0, 0, W, H);
      return t => {
        ctx.globalCompositeOperation = 'destination-out';
        ctx.fillStyle = 'rgba(0,0,0,0.07)'; ctx.fillRect(0, 0, W, H);
        ctx.globalCompositeOperation = 'source-over';
        ctx.fillStyle = color; ctx.globalAlpha = 0.5;
        for (const p of ps) {
          const a = Math.sin(p.x * 0.0021 + t * 0.15) * 2.2 + Math.cos(p.y * 0.0027 - t * 0.1) * 2.2;
          let sp = 1.4;
          if (L === 4 && ptr.inside) { const d = Math.hypot(p.x - ptr.x, p.y - ptr.y); if (d < 220) sp += (1 - d / 220) * 4; }
          p.x += Math.cos(a) * sp; p.y += Math.sin(a) * sp;
          if (p.x < 0 || p.x > W || p.y < 0 || p.y > H) { p.x = rnd(0, W); p.y = rnd(0, H); }
          ctx.fillRect(p.x, p.y, 1.6, 1.6);
        }
        ctx.globalAlpha = 1;
      };
    },
    dotgrid(ctx, color, L) {
      const gap = 40, amp = L === 4 ? 14 : 7;
      return t => {
        ctx.clearRect(0, 0, W, H);
        ctx.fillStyle = color;
        const wave = t * 900;
        for (let x = gap / 2; x < W; x += gap) for (let y = gap / 2; y < H; y += gap) {
          const dc = Math.hypot(x - W / 2, y - H / 2);
          let r = 1.4, ox = 0, oy = 0;
          const w = Math.exp(-((dc - wave) ** 2) / 9000);
          r += w * 3;
          if (ptr.inside) {
            const dx = x - ptr.x, dy = y - ptr.y, d = Math.hypot(dx, dy);
            if (d < 220) { const k = (1 - d / 220); ox = dx / (d || 1) * amp * k; oy = dy / (d || 1) * amp * k; r += k * 2.2; }
          }
          ctx.globalAlpha = 0.22 + w * 0.6;
          ctx.beginPath(); ctx.arc(x + ox, y + oy, r, 0, 7); ctx.fill();
        }
        ctx.globalAlpha = 1;
      };
    },
    grain(ctx, color, L) {
      const frames = Array.from({ length: 4 }, () => {
        const c = document.createElement('canvas'); c.width = c.height = 256;
        const x = c.getContext('2d'), d = x.createImageData(256, 256);
        for (let i = 0; i < d.data.length; i += 4) { const v = Math.random() * 255; d.data[i] = d.data[i + 1] = d.data[i + 2] = v; d.data[i + 3] = 255; }
        x.putImageData(d, 0, 0); return ctx.createPattern(c, 'repeat');
      });
      let last = -1;
      return t => {
        const f = Math.floor(t * 12) % frames.length; if (f === last) return; last = f;
        ctx.clearRect(0, 0, W, H); ctx.globalAlpha = L === 4 ? 0.09 : 0.06;
        ctx.fillStyle = frames[f]; ctx.fillRect(0, 0, W, H); ctx.globalAlpha = 1;
      };
    },
  };

  // ---------- SVG flow packets on [data-flow] paths ----------
  const flows = new WeakMap();
  function flowStart(s) {
    const L = effLevel();
    s.querySelectorAll('[data-flow]').forEach(p => {
      p.classList.toggle('kx-march', L >= 2);
      if (L < 3 || !(p instanceof SVGGeometryElement)) return;
      const svg = p.ownerSVGElement; let len = 0; try { len = p.getTotalLength(); } catch {}
      if (!svg || !len) return;
      const n = +(p.dataset.packets || (L === 4 ? 3 : 1));
      const dots = Array.from({ length: n }, () => {
        const c = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
        c.setAttribute('r', p.dataset.packetSize || 6); c.setAttribute('class', 'kx-packet');
        p.parentNode.insertBefore(c, p.nextSibling); return c;
      });
      const speed = parseFloat(p.dataset.speed || 1) * (L === 4 ? 1.6 : 1);
      const t0 = performance.now();
      const loop = now => {
        const t = (now - t0) / 1000;
        dots.forEach((d, k) => {
          const u = ((t * speed * 0.35) + k / n) % 1;
          const pt = p.getPointAtLength((p.dataset.reverse !== undefined ? 1 - u : u) * len);
          d.setAttribute('cx', pt.x); d.setAttribute('cy', pt.y);
          d.setAttribute('opacity', Math.sin(u * Math.PI).toFixed(3));
        });
        flows.get(p).raf = requestAnimationFrame(loop);
      };
      flows.set(p, { dots, raf: requestAnimationFrame(loop) });
    });
  }
  function flowStop(s) {
    s.querySelectorAll('[data-flow]').forEach(p => {
      const f = flows.get(p); if (!f) return;
      cancelAnimationFrame(f.raf); f.dots.forEach(d => d.remove()); flows.delete(p);
    });
  }

  // ---------- navigation ----------
  function go(n, dir = n > idx ? 1 : -1, toStep = 0) {
    n = clamp(n, 0, slides.length - 1);
    const from = slides[idx], to = slides[n];
    if (n === idx && toStep === step) return;
    if (G && busy) G.globalTimeline.getChildren(false, false, true).forEach(t => t.progress(1));
    fxStop(from); flowStop(from);
    finishEnter();
    const prev = idx;
    if (prev !== n) stopHook(from);
    idx = n; step = toStep;
    if (prev === n) { applySteps(to, step); return chrome(); }
    transition(from, to, dir, n, delay => {
      enter(to, delay);
      fxStart(to); flowStart(to);
    });
    if (location.hash !== `#${n + 1}`) history.replaceState(null, '', `#${n + 1}`);
    chrome();
  }
  function next() {
    const st = stepsOf(slides[idx]); const nx = st.find(v => v > step);
    if (nx !== undefined) { step = nx; revealStep(slides[idx], nx); return chrome(); }
    if (idx < slides.length - 1) go(idx + 1, 1);
  }
  function prev() {
    const st = stepsOf(slides[idx]); const pv = [...st].reverse().find(v => v < step);
    if (step > 0) { step = pv ?? 0; applySteps(slides[idx], step); return chrome(); }
    if (idx > 0) { const p = slides[idx - 1]; const ps = stepsOf(p); go(idx - 1, -1, ps.length ? ps[ps.length - 1] : 0); }
  }
  function replay() {
    const s = slides[idx];
    fxStop(s); flowStop(s);
    enter(s, 0); fxStart(s); flowStart(s);
  }

  addEventListener('keydown', e => {
    if (e.target.closest?.('input,textarea,select,[contenteditable="true"]') || e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key;
    if (['ArrowRight', 'ArrowDown', 'PageDown', ' ', 'Enter'].includes(k)) { e.preventDefault(); next(); }
    else if (['ArrowLeft', 'ArrowUp', 'PageUp', 'Backspace'].includes(k)) { e.preventDefault(); prev(); }
    else if (k === 'Home') go(0);
    else if (k === 'End') go(slides.length - 1);
    else if ((k === 'm' || k === 'M') && ALLOWED.length > 1) {
      const i = ALLOWED.indexOf(level), n = ALLOWED.length;
      setLevel(ALLOWED[((i < 0 ? 0 : i) + (e.shiftKey ? n - 1 : 1)) % n]);
    }
    else if (k === 'f' || k === 'F') (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen?.());
    else if (k === 'o' || k === 'O') toggleOverlay('overview');
    else if (k === 'n' || k === 'N') toggleOverlay('notes');
    else if (k === 'p' || k === 'P') presenter();
    else if (k === 'Escape') { closeOverlays(); clearFocus(); }
  });
  let touchX = null;
  stage.addEventListener('touchstart', e => { touchX = e.touches[0].clientX; }, { passive: true });
  stage.addEventListener('touchend', e => {
    if (touchX === null) return;
    const dx = e.changedTouches[0].clientX - touchX; touchX = null;
    if (Math.abs(dx) > 50) (dx < 0 ? next() : prev());
  });
  addEventListener('hashchange', () => { const n = parseInt(location.hash.slice(1), 10) - 1; if (n >= 0 && n !== idx) go(n); });

  // ---------- interactions ----------
  document.querySelectorAll('[data-tabs]').forEach(box => {
    const tabs = [...box.querySelectorAll('[data-tab]')], panes = [...box.querySelectorAll('[data-pane]')];
    const show = (id, animate) => {
      tabs.forEach(t => t.classList.toggle('is-on', t.dataset.tab === id));
      panes.forEach(p => {
        const on = p.dataset.pane === id; p.hidden = !on;
        if (on && animate && G && effLevel() >= 1) G.from(p, { opacity: 0, y: 16, duration: 0.5, ease: 'expo.out' });
      });
    };
    tabs.forEach(t => t.addEventListener('click', () => show(t.dataset.tab, true)));
    if (tabs[0]) show(tabs[0].dataset.tab, false);
  });

  function clearFocus(scope = stage) {
    scope.querySelectorAll('.is-focus,.is-dim,.is-lit').forEach(e => e.classList.remove('is-focus', 'is-dim', 'is-lit'));
    scope.querySelectorAll('[data-detail]').forEach(d => { d.hidden = d.dataset.detail !== 'default'; });
  }
  stage.addEventListener('click', e => {
    const node = e.target.closest('[data-node]');
    if (!node) return;
    const s = node.closest('.slide'), id = node.dataset.node;
    const was = node.classList.contains('is-focus');
    clearFocus(s);
    if (was) return;
    s.querySelectorAll('[data-node]').forEach(n => n.classList.toggle('is-dim', n !== node && !(n.dataset.links || '').split(/\s+/).includes(id) && !(node.dataset.links || '').split(/\s+/).includes(n.dataset.node)));
    node.classList.add('is-focus');
    s.querySelectorAll('[data-edge]').forEach(ed => {
      const on = ed.dataset.edge.split(/\s+/).includes(id);
      ed.classList.toggle('is-lit', on); ed.classList.toggle('is-dim', !on);
    });
    s.querySelectorAll('[data-detail]').forEach(d => {
      const on = d.dataset.detail === id; d.hidden = !on;
      if (on && G && effLevel() >= 1) G.from(d, { opacity: 0, x: 24, duration: 0.5, ease: 'expo.out' });
    });
  });
  slides.forEach(s => clearFocus(s));

  // Node boxes from the g's transform and its rect, so this works before layout and on hidden slides.
  function nodeBox(svg, id) {
    const g = svg.querySelector(`[data-node="${CSS.escape(id)}"]`), r = g?.querySelector('rect');
    if (!r) return null;
    const m = g.transform.baseVal.consolidate()?.matrix, x = (m?.e || 0) + (+r.getAttribute('x') || 0), y = (m?.f || 0) + (+r.getAttribute('y') || 0);
    const w = +r.getAttribute('width'), h = +r.getAttribute('height');
    return { x, y, w, h, cx: x + w / 2, cy: y + h / 2 };
  }
  // An edge with no d is routed box to box, so agents never hand-place endpoints.
  document.querySelectorAll('svg path[data-edge]:not([d])').forEach(p => {
    const [a, b] = p.dataset.edge.split(/\s+/).map(id => nodeBox(p.ownerSVGElement, id));
    if (!a || !b) return;
    const dx = b.cx - a.cx, dy = b.cy - a.cy;
    if (Math.abs(dx) * a.h >= Math.abs(dy) * a.w) {
      const sx = dx > 0 ? a.x + a.w : a.x, ex = dx > 0 ? b.x : b.x + b.w, mx = (sx + ex) / 2;
      p.setAttribute('d', `M${sx} ${a.cy} C ${mx} ${a.cy}, ${mx} ${b.cy}, ${ex} ${b.cy}`);
    } else {
      const sy = dy > 0 ? a.y + a.h : a.y, ey = dy > 0 ? b.y : b.y + b.h, my = (sy + ey) / 2;
      p.setAttribute('d', `M${a.cx} ${sy} C ${a.cx} ${my}, ${b.cx} ${my}, ${b.cx} ${ey}`);
    }
  });

  const tip = document.createElement('div'); tip.className = 'kx-tip'; stage.appendChild(tip);
  stage.addEventListener('pointerover', e => { const t = e.target.closest('[data-tip]'); if (t) { tip.textContent = t.dataset.tip; tip.classList.add('is-on'); } });
  stage.addEventListener('pointerout', e => { if (e.target.closest('[data-tip]')) tip.classList.remove('is-on'); });
  stage.addEventListener('pointermove', () => { tip.style.transform = `translate(${ptr.x + 18}px, ${ptr.y + 18}px)`; }, { passive: true });

  stage.addEventListener('pointermove', e => {
    const L = effLevel(); if (!G || L < 3) return;
    const t = e.target.closest('[data-tilt]');
    if (t) {
      const r = t.getBoundingClientRect(), x = (e.clientX - r.left) / r.width - 0.5, y = (e.clientY - r.top) / r.height - 0.5;
      const a = L === 4 ? 12 : 7;
      G.to(t, { rotationY: x * a, rotationX: -y * a, transformPerspective: 900, duration: 0.6, ease: 'power3.out', overwrite: 'auto' });
    }
    if (L === 4) {
      const m = e.target.closest('[data-magnetic]');
      if (m) { const r = m.getBoundingClientRect(); G.to(m, { x: (e.clientX - r.left - r.width / 2) / scale * 0.3, y: (e.clientY - r.top - r.height / 2) / scale * 0.3, duration: 0.5, ease: 'power3.out', overwrite: 'auto' }); }
    }
  }, { passive: true });
  stage.addEventListener('pointerout', e => {
    if (!G) return;
    const t = e.target.closest('[data-tilt],[data-magnetic]');
    if (t && !t.contains(e.relatedTarget)) G.to(t, { rotationX: 0, rotationY: 0, x: 0, y: 0, duration: 0.8, ease: 'elastic.out(1, 0.5)' });
  });

  // ---------- chrome: progress, counter, motion HUD, overlays ----------
  const ui = document.createElement('div');
  ui.className = 'kx-ui';
  ui.innerHTML = `
    <div class="kx-progress"><i></i></div>
    <div class="kx-count"></div>
    <div class="kx-hud" role="group" aria-label="Motion level">
      <span class="kx-hud-label">Motion</span>
      ${ALLOWED.map(i => `<button type="button" data-level="${i}" title="${LEVELS[i]} (M to cycle)"><b></b><span>${LEVELS[i]}</span></button>`).join('')}
    </div>
    <div class="kx-toast"></div>
    <div class="kx-overlay" data-overlay="overview"><div class="kx-ov-grid"></div></div>
    <div class="kx-overlay" data-overlay="notes"><div class="kx-notes"></div></div>`;
  document.body.appendChild(ui);
  if (document.body.dataset.chrome === 'off') ui.classList.add('kx-chrome-off');
  if (ALLOWED.length < 2) ui.classList.add('kx-one-level');
  const $ = s => ui.querySelector(s);
  $('.kx-hud').addEventListener('click', e => { const b = e.target.closest('[data-level]'); if (b) setLevel(+b.dataset.level); });
  $('.kx-ov-grid').innerHTML = slides.map((s, i) => `<button type="button" data-go="${i}"><em>${String(i + 1).padStart(2, '0')}</em>${titleOf(s, i)}</button>`).join('');
  $('.kx-ov-grid').addEventListener('click', e => { const b = e.target.closest('[data-go]'); if (b) { closeOverlays(); go(+b.dataset.go); } });

  let hudT = 0;
  // The motion HUD shows only while hovered (CSS), plus a short peek after M changes the level.
  function peekHud() { const h = $('.kx-hud'); h.classList.add('is-peek'); clearTimeout(h._t); h._t = setTimeout(() => h.classList.remove('is-peek'), 1600); }
  function hudWake() { ui.classList.add('is-awake'); clearTimeout(hudT); hudT = setTimeout(() => ui.classList.remove('is-awake'), 2200); }
  function toast(msg) { const t = $('.kx-toast'); t.textContent = msg; t.classList.add('is-on'); clearTimeout(t._t); t._t = setTimeout(() => t.classList.remove('is-on'), 1400); }

  function notesHTML(i) {
    const s = slides[i];
    const n = s.querySelector('.notes')?.innerHTML || '<p class="kx-dim">No speaker notes.</p>';
    const ids = (s.dataset.claims || '').split(/\s+/).filter(Boolean);
    const cites = ids.map(id => {
      const c = sources.claims?.[id]; if (!c) return '';
      const src = sources.sources?.[c.source];
      return `<li><b>${id}</b> ${src ? src.ref : c.source}${c.locator ? ' · ' + c.locator : ''}</li>`;
    }).join('');
    return `<h4>${String(i + 1).padStart(2, '0')} · ${titleOf(s, i)}</h4>${n}${cites ? `<ul class="kx-cites">${cites}</ul>` : ''}`;
  }
  function chrome() {
    $('.kx-progress i').style.transform = `scaleX(${(idx + 1) / slides.length})`;
    $('.kx-count').textContent = `${String(idx + 1).padStart(2, '0')} / ${String(slides.length).padStart(2, '0')}`;
    ui.querySelectorAll('[data-level]').forEach(b => b.classList.toggle('is-on', +b.dataset.level <= level));
    ui.querySelectorAll('[data-level]').forEach(b => b.classList.toggle('is-cur', +b.dataset.level === level));
    $('.kx-notes').innerHTML = notesHTML(idx);
    ui.querySelectorAll('[data-go]').forEach(b => b.classList.toggle('is-cur', +b.dataset.go === idx));
    slides.forEach((s, i) => s.setAttribute('aria-hidden', i === idx ? 'false' : 'true'));
    syncPresenter();
  }
  function toggleOverlay(name) {
    const o = ui.querySelector(`[data-overlay="${name}"]`);
    const on = !o.classList.contains('is-on'); closeOverlays(); o.classList.toggle('is-on', on);
  }
  function closeOverlays() { ui.querySelectorAll('.kx-overlay').forEach(o => o.classList.remove('is-on')); }

  function setLevel(L) {
    level = snap(clamp(L, 0, 4)); store.set('kx-motion', level);
    peekHud();
    document.body.dataset.level = level;
    toast(`Motion · ${LEVELS[level]}${!G && level > 1 ? ' (GSAP not loaded, capped at Minimal)' : ''}`);
    chrome(); replay();
  }

  // ---------- presenter window ----------
  let pw = null, t0 = Date.now();
  function presenter() {
    pw = window.open('', 'kx-presenter', 'width=900,height=640');
    if (!pw) return toast('Popup blocked. Allow popups for presenter view.');
    pw.document.title = 'Presenter';
    pw.document.body.innerHTML = '<div id="p"></div>';
    const st = pw.document.createElement('style');
    st.textContent = 'body{margin:0;padding:32px;background:#111;color:#eee;font:18px/1.55 ui-sans-serif,system-ui,sans-serif}h4{font-size:28px;margin:0 0 16px}#t{font:600 44px ui-monospace,monospace;color:#9f9}.nx{color:#888;margin-top:24px;border-top:1px solid #333;padding-top:16px}.kx-cites{color:#aaa;font-size:14px}';
    pw.document.head.appendChild(st);
    pw.addEventListener('keydown', e => dispatchEvent(new KeyboardEvent('keydown', { key: e.key, shiftKey: e.shiftKey })));
    t0 = Date.now(); syncPresenter();
  }
  function syncPresenter() {
    if (!pw || pw.closed) return;
    const el = pw.document.getElementById('p'); if (!el) return;
    const m = Math.floor((Date.now() - t0) / 60000), s = Math.floor((Date.now() - t0) / 1000) % 60;
    el.innerHTML = `<div id="t">${m}:${String(s).padStart(2, '0')}</div>${notesHTML(idx)}<div class="nx">Next: ${idx + 1 < slides.length ? titleOf(slides[idx + 1], idx + 1) : 'End'}</div>`;
  }
  setInterval(syncPresenter, 1000);

  // ---------- self-audit (?audit) ----------
  function lum(rgb) {
    const [r, g, b] = rgb.map(v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }
  const rgba = s => { const m = s.match(/[\d.]+/g); return m ? m.map(Number) : null; };
  function bgOf(el) {
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.backgroundImage !== 'none') return null;
      const c = rgba(cs.backgroundColor);
      if (c && (c[3] === undefined || c[3] > 0.9)) return c.slice(0, 3);
    }
    return rgba(getComputedStyle(document.body).backgroundColor)?.slice(0, 3) || [255, 255, 255];
  }
  const desc = el => (el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + (el.classList.length ? '.' + [...el.classList].filter(c => !c.startsWith('kx-')).join('.') : '')) + ` "${(el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 48)}"`;

  function auditSlide(s, i) {
    const issues = [];
    const sr = s.getBoundingClientRect(), tol = 2 * scale;
    const flagged = new Set(), texts = [];
    for (const el of s.querySelectorAll('*')) {
      if (el.closest('.notes,.kx-fx,[data-bleed],[hidden]') || (el.ownerSVGElement && el.tagName.toLowerCase() !== 'text')) continue;
      const cs = getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || +cs.opacity === 0) continue;
      const r = el.getBoundingClientRect(); if (!r.width || !r.height) continue;
      const hasText = [...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim());
      if ((r.left < sr.left - tol || r.top < sr.top - tol || r.right > sr.right + tol || r.bottom > sr.bottom + tol) && !flagged.has(el.parentElement)) {
        flagged.add(el); issues.push({ type: 'offstage', el: desc(el) }); el.classList.add('kx-bad');
      } else if (flagged.has(el.parentElement)) flagged.add(el);
      if (/(hidden|clip|auto|scroll)/.test(cs.overflowX + cs.overflowY) && el.tagName !== 'svg' && (el.scrollHeight > el.clientHeight + 2 || el.scrollWidth > el.clientWidth + 2)) {
        issues.push({ type: 'clipped', el: desc(el) }); el.classList.add('kx-bad');
      }
      if (!hasText) continue;
      // SVG font-size is in viewBox units; the screen CTM gives the size as rendered on the 1920 stage.
      const fs = parseFloat(cs.fontSize) * (el.ownerSVGElement ? (el.getScreenCTM()?.a || scale) / scale : 1);
      if (fs < 18) issues.push({ type: 'tiny-text', el: desc(el), px: Math.round(fs * 10) / 10 });
      const fg = rgba(cs.color), bg = bgOf(el);
      if (fg && bg && (fg[3] === undefined || fg[3] > 0.5)) {
        const a = lum(fg.slice(0, 3)), b = lum(bg), ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        const need = fs >= 36 || (fs >= 28 && +cs.fontWeight >= 600) ? 3 : 4.5;
        if (ratio < need && !el.closest('[data-deco]')) issues.push({ type: 'low-contrast', el: desc(el), ratio: +ratio.toFixed(2), need });
      }
      const pad = fs >= 60 ? fs * 0.14 : 0;  // tight display leading makes line boxes overlap while glyphs do not
      texts.push({ el, r: { left: r.left, right: r.right, top: r.top + pad * scale, bottom: r.bottom - pad * scale, width: r.width, height: Math.max(1, r.height - 2 * pad * scale) } });
    }
    for (let a = 0; a < texts.length; a++) for (let b = a + 1; b < texts.length; b++) {
      const A = texts[a], B = texts[b];
      if (A.el.contains(B.el) || B.el.contains(A.el) || A.el.closest('[data-deco]') || B.el.closest('[data-deco]')) continue;
      const w = Math.min(A.r.right, B.r.right) - Math.max(A.r.left, B.r.left), h = Math.min(A.r.bottom, B.r.bottom) - Math.max(A.r.top, B.r.top);
      if (w > 0 && h > 0 && w * h > 0.15 * Math.min(A.r.width * A.r.height, B.r.width * B.r.height)) {
        issues.push({ type: 'overlap', el: desc(A.el), with: desc(B.el) }); A.el.classList.add('kx-bad'); B.el.classList.add('kx-bad');
      }
    }
    for (const p of s.querySelectorAll('svg path[data-edge]')) {
      const boxes = p.dataset.edge.split(/\s+/).map(id => nodeBox(p.ownerSVGElement, id)).filter(Boolean);
      if (boxes.length < 2 || !p.getTotalLength) continue;
      const near = pt => boxes.some(b => Math.max(b.x - pt.x, 0, pt.x - b.x - b.w) ** 2 + Math.max(b.y - pt.y, 0, pt.y - b.y - b.h) ** 2 <= 16 ** 2);
      if (!near(p.getPointAtLength(0)) || !near(p.getPointAtLength(p.getTotalLength())))
        issues.push({ type: 'loose-edge', el: `edge "${p.dataset.edge}"`, fix: 'an end does not touch its node; delete the d attribute and the engine routes it' });
    }
    const OPEN = /statement|poster|quote|giant|cover|section/;
    if (!OPEN.test(s.dataset.layout || '') && !s.hasAttribute('data-open')) {
      let bottom = 0;
      for (const el of s.querySelectorAll('h1,h2,h3,p,li,svg,img,video,canvas:not(.kx-fx),table,pre,figure,[data-node],[data-anim],[data-move]')) {
        if (el.closest('.notes,[data-deco],.kx-fx,[hidden]')) continue;
        const r = el.getBoundingClientRect(); if (!r.height) continue;
        bottom = Math.max(bottom, (r.bottom - sr.top) / scale);
      }
      if (bottom && bottom < H * 0.66) issues.push({ type: 'empty-bottom', el: `content ends at ${Math.round(bottom)}px of ${H}`, fix: 'scale the main visual up, move content down, or add data-open if the space is intended' });
    }
    return { slide: i + 1, id: s.id || null, title: titleOf(s, i), layout: s.dataset.layout || null, issues };
  }
  async function audit() {
    await Promise.race([document.fonts.ready, new Promise(r => setTimeout(r, 4000))]);
    const report = [];
    slides.forEach((s, i) => {
      slides.forEach(x => x.classList.toggle('is-active', x === s));
      applySteps(s, Infinity);
      runHook(s, 0);
      report.push(auditSlide(s, i));
    });
    slides.forEach((x, i) => x.classList.toggle('is-active', i === idx));
    const json = JSON.stringify({ slides: slides.length, report }).replace(/</g, '\\u003c');
    const tag = document.createElement('script'); tag.type = 'application/json'; tag.id = 'kx-audit'; tag.textContent = json;
    document.body.appendChild(tag);
    window.__kxAudit = JSON.parse(json);
    console.log('[stagecraft audit]', window.__kxAudit);
  }

  // ---------- boot ----------
  document.body.dataset.level = level;
  document.body.classList.toggle('kx-print-static', qs.has('static'));
  const start = parseInt(location.hash.slice(1), 10) - 1;
  idx = clamp(isNaN(start) ? 0 : start, 0, slides.length - 1);
  slides.forEach((s, i) => { s.classList.toggle('is-active', i === idx); applySteps(s, 0); });
  chrome();
  if (AUDIT) { audit(); return; }
  if (qs.has('static')) { slides.forEach(s => { applySteps(s, Infinity); runHook(s, 0); }); return; }
  Promise.race([document.fonts.ready, new Promise(r => setTimeout(r, 1500))]).then(() => {
    enter(slides[idx], 0.15); fxStart(slides[idx]); flowStart(slides[idx]);
  });
  window.KX = { go, next, prev, setLevel, get level() { return level; }, slides };
})();
