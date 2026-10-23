(() => {
  if (parent === window) return;
  const post = m => parent.postMessage(Object.assign({ stage: 1 }, m), '*');
  const ACCENT = '#e9783e';
  let edit = false, sel = null, hov = null, selBox = null, hovBox = null;

  // slide number
  let h = null;
  setInterval(() => {
    if (location.hash !== h) { h = location.hash; post({ slide: parseInt(h.slice(1), 10) || 1 }); draw(); setTimeout(draw, 700); }
  }, 200);

  // edit overlays: fixed boxes in viewport space (the stage is CSS-scaled)
  const box = css => {
    const d = document.createElement('div');
    d.style.cssText = 'position:fixed;pointer-events:none;z-index:2147483647;display:none;box-sizing:border-box;' + css;
    document.body.appendChild(d);
    return d;
  };
  const place = (d, el) => {
    if (!el || !el.closest('.slide.is-active')) { d.style.display = 'none'; return; }
    const r = el.getBoundingClientRect();
    d.style.cssText += `;display:block;left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px`;
  };
  const byIndex = i => (i == null ? null : document.querySelector(`[data-kx-i="${i}"]`));
  const pick = t => {
    const el = t && t.closest && t.closest('[data-kx-i]');
    return el && el.closest('.slide.is-active') ? el : null;
  };
  const info = el => {
    const cs = getComputedStyle(el);
    return {
      i: +el.dataset.kxI,
      tag: el.tagName.toLowerCase(),
      text: (el.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 60),
      style: {
        fontFamily: cs.fontFamily, fontSize: cs.fontSize, color: cs.color, fontWeight: cs.fontWeight,
        fontStyle: cs.fontStyle, textDecorationLine: cs.textDecorationLine, letterSpacing: cs.letterSpacing,
        lineHeight: cs.lineHeight, textAlign: cs.textAlign,
      },
    };
  };
  const draw = () => {
    if (!edit) { if (hovBox) hovBox.style.display = 'none'; if (selBox) selBox.style.display = 'none'; return; }
    if (hov) place(hovBox || (hovBox = box(`border:1.5px solid ${ACCENT};`)), hov);
    else if (hovBox) hovBox.style.display = 'none';
    const el = byIndex(sel);
    if (el) place(selBox || (selBox = box(`border:2px solid ${ACCENT};background:${ACCENT}14;`)), el);
    else if (selBox) selBox.style.display = 'none';
  };
  addEventListener('resize', draw);
  addEventListener('hashchange', draw);

  addEventListener('message', e => {
    if (e.source !== parent || !e.data || !e.data.stage) return;
    if ('edit' in e.data) { edit = !!e.data.edit; if (!edit) hov = null; draw(); }
    if ('select' in e.data) {
      sel = e.data.select; draw();
      // Echo the element's current computed style, so the parent's placeholders follow each rebuild.
      const el = byIndex(sel);
      if (edit && el) post({ select: info(el), refresh: true });
    }
  });

  addEventListener('pointermove', e => {
    if (!edit) return;
    const el = pick(e.target);
    if (el !== hov) { hov = el; draw(); }
  }, true);

  addEventListener('click', e => {
    if (!edit) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    const el = pick(e.target);
    if (!el) return;
    sel = +el.dataset.kxI;
    draw();
    post({ select: info(el) });
  }, true);

  // keys: capture phase so Escape is seen before the engine closes overlays
  addEventListener('keydown', e => {
    if (e.key === 'F5') {
      e.preventDefault(); e.stopPropagation();
      post({ key: e.shiftKey ? 'shift+f5' : 'f5' });
    } else if (e.key === 'Escape') {
      if (edit) { e.preventDefault(); e.stopImmediatePropagation(); post({ key: 'edit-off' }); }
      else post({ key: 'escape' });
    } else if ((e.ctrlKey || e.metaKey) && ['s', 'o', 'n'].includes(e.key.toLowerCase())) {
      e.preventDefault();
      post({ key: 'ctrl+' + e.key.toLowerCase() });
    } else if ((e.ctrlKey || e.metaKey) && ['z', 'y'].includes(e.key.toLowerCase())) {
      e.preventDefault();
      post({ key: e.key.toLowerCase() === 'y' ? 'ctrl+y' : e.shiftKey ? 'ctrl+shift+z' : 'ctrl+z' });
    }
  }, true);
})();
