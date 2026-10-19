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
