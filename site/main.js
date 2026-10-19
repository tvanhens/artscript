(() => {
  const DECLS = 'use palette style finish def rule let light brush stamp anim each if else assert expect slot play ramp color fx mask'.split(' ');
  const art = [
    { kind: 'hex', regex: /#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{3})\b/y },
    { kind: 'comment', regex: /#.*/y },
    { kind: 'str', regex: /"[^"\n]*"/y },
    { kind: 'decl', regex: new RegExp(`(?:@target|(?:${DECLS.join('|')}))(?![\\w:])`, 'y'), start: true },
    { kind: 'verb', regex: /[A-Za-z_]\w*(?![\w:])/y, start: true },
    { kind: 'key', regex: /(?<![\w.])[A-Za-z_]\w*(?=:)/y },
    { kind: 'ref', regex: /(?<![\w.])p\.[\w.]+|@[A-Za-z_]\w*|(?<![\w)\]}/.])\.[A-Za-z_]\w*(?:\.\w+)*/y },
    { kind: 'num', regex: /(?<![\w.])-?(?:\d+x\d+|(?:\d+\.?\d*|\.\d+)(?:ms|s)?)(?!\w)/y },
    { kind: 'punct', regex: /->|[[\]{}(),]/y },
  ];
  const sh = [
    { kind: 'comment', regex: /#.*/y },
    { kind: 'verb', regex: /[A-Za-z_][\w-]*/y, start: true },
    { kind: 'key', regex: /(?<![\w-])--?[A-Za-z][\w-]*/y },
  ];
  const tables = { art, sh, diff: art };

  const chip = (hex) => {
    const c = document.createElement('span');
    c.className = 't-chip';
    c.setAttribute('aria-hidden', 'true');
    c.style.background = hex;
    return c;
  };

  const tokenize = (text, rules, state, withChip) => {
    const out = document.createDocumentFragment();
    let plain = '';
    const flush = () => { if (plain) { out.append(plain); plain = ''; } };
    let i = 0;
    while (i < text.length) {
      const ch = text[i];
      if (ch === '\n') { plain += ch; state.first = true; i += 1; continue; }
      let hit = null;
      for (const rule of rules) {
        if (rule.start && !state.first) continue;
        rule.regex.lastIndex = i;
        const m = rule.regex.exec(text);
        if (m && m[0]) { hit = { rule, text: m[0] }; break; }
      }
      if (!hit) {
        plain += ch;
        if (!/\s/.test(ch)) state.first = false;
        i += 1;
        continue;
      }
      flush();
      const span = document.createElement('span');
      span.className = `t-${hit.rule.kind}`;
      span.textContent = hit.text;
      if (hit.rule.kind === 'hex' && withChip) span.append(chip(hit.text));
      out.append(span);
      state.first = false;
      i += hit.text.length;
    }
    flush();
    return out;
  };

  const textNodes = (root) => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    return nodes;
  };

  const run = (root, rules, state) => {
    for (const node of textNodes(root)) {
      if (node.parentElement.closest('.t-sign')) continue;
      if (node.parentElement.closest('[data-token]')) { state.first = false; continue; }
      const withChip = !node.parentElement.closest('mark');
      node.replaceWith(tokenize(node.data, rules, state, withChip));
    }
  };

  const splitLines = (code) => {
    const lines = [];
    let line = document.createElement('span');
    const close = () => { lines.push(line); line = document.createElement('span'); };
    for (const node of [...code.childNodes]) {
      if (node.nodeType !== Node.TEXT_NODE) { line.append(node); continue; }
      node.data.split('\n').forEach((part, k) => {
        if (k) close();
        if (part) line.append(part);
      });
    }
    if (line.childNodes.length) close();
    code.replaceChildren(...lines);
    return lines;
  };

  const diffClass = (line) => {
    const head = line.textContent;
    if (/^(---|\+\+\+|@@)/.test(head)) return 'meta';
    if (head[0] === '+') return 'add';
    if (head[0] === '-') return 'del';
    return 'ctx';
  };

  const highlight = (code, lang) => {
    const rules = tables[lang];
    if (!rules) return;
    for (const line of splitLines(code)) {
      line.classList.add('lw');
      line.style.setProperty('--hang', `${line.textContent.match(/^ */)[0].length + 4}ch`);
      if (lang !== 'diff') { run(line, rules, { first: true }); continue; }
      const kind = diffClass(line);
      line.classList.add('ln', `ln--${kind}`);
      if (kind === 'meta') continue;
      const first = line.firstChild;
      if (kind !== 'ctx' && first.nodeType === Node.TEXT_NODE) {
        first.splitText(1);
        const sign = document.createElement('span');
        sign.className = 't-sign';
        sign.textContent = first.data;
        first.replaceWith(sign);
        run(line, rules, { first: true });
      } else {
        run(line, rules, { first: true });
      }
    }
  };

  document.querySelectorAll('pre[data-lang]').forEach((pre) => {
    const code = pre.querySelector('code');
    if (code) highlight(code, pre.dataset.lang);
  });
})();

(() => {
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;

  const shutter = document.querySelector('.shutter');
  if (shutter) requestAnimationFrame(() => requestAnimationFrame(() => shutter.classList.add('is-in')));

  const revealed = document.querySelectorAll('[data-reveal]');
  if (reduce || !('IntersectionObserver' in window)) {
    revealed.forEach((el) => el.classList.add('is-in'));
  } else {
    const io = new IntersectionObserver((entries) => {
      let step = 0;
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        e.target.style.setProperty('--d', `${Math.min(step, 4) * 70}ms`);
        e.target.classList.add('is-in');
        io.unobserve(e.target);
        step += 1;
      }
    }, { rootMargin: '0px 0px -10% 0px', threshold: 0.08 });
    revealed.forEach((el) => io.observe(el));
  }

  const form = document.getElementById('demo-form');
  if (!form) return;
  const img = document.getElementById('demo-img');
  const out = document.getElementById('demo-out');
  const options = {
    wing: ['luna', 'dusk', 'ember', 'bone', 'rosy'],
    pattern: ['1', '2', '3', '4'],
    tails: ['on', 'off'],
  };
  const fileFor = (s) => `demo/moth_${s.wing}_${s.pattern}_${s.tails}.png`;
  const srcFor = (s) => `art/moths/assets/${fileFor(s)}`;
  const read = () => Object.fromEntries(Object.keys(options).map((k) => [k, form.elements[k].value]));

  const warmed = new Set();
  const warm = (s) => {
    const src = srcFor(s);
    if (warmed.has(src)) return;
    warmed.add(src);
    new Image().src = src;
  };
  const warmNeighbours = (s) => {
    const idle = window.requestIdleCallback || ((fn) => setTimeout(fn, 200));
    idle(() => {
      for (const [key, values] of Object.entries(options)) {
        for (const v of values) warm({ ...s, [key]: v });
      }
    });
  };

  let current = read();
  let pending = 0;
  form.addEventListener('change', (event) => {
    const next = read();
    const changed = event.target.name;
    const token = form.querySelector(`[data-token="${changed}"]`);
    token.textContent = next[changed];
    token.classList.add('flash');
    requestAnimationFrame(() => requestAnimationFrame(() => token.classList.remove('flash')));
    out.textContent = fileFor(next);

    const ticket = ++pending;
    const loader = new Image();
    img.classList.add('is-loading');
    loader.onload = loader.onerror = () => {
      if (ticket !== pending) return;
      img.src = loader.src;
      img.alt = `The demo moth with wing ${next.wing}, pattern ${next.pattern}, tails ${next.tails}`;
      img.classList.remove('is-loading');
    };
    loader.src = srcFor(next);
    current = next;
    warmNeighbours(current);
  });

  form.addEventListener('submit', (event) => event.preventDefault());
  form.addEventListener('pointerover', (event) => {
    const input = event.target.closest('.choice')?.querySelector('input');
    if (input) warm({ ...current, [input.name]: input.value });
  });

  const demo = document.getElementById('demo');
  new IntersectionObserver((entries, obs) => {
    if (entries.some((e) => e.isIntersecting)) { warmNeighbours(current); obs.disconnect(); }
  }, { rootMargin: '400px 0px' }).observe(demo);
})();
