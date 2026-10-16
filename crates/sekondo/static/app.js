// htmx would inject an inline <style> for indicators, which the CSP forbids.
// The indicator styles live in app.css instead.
htmx.config.includeIndicatorStyles = false;
// No eval: the CSP forbids it, and nothing here needs hx-on or hx-vals javascript.
htmx.config.allowEval = false;

// The character filter is a list of checkboxes, but the URL carries one comma-separated `chars`
// value (and none at all when every character is selected), plus the sorting only when it is not
// the default. A plain form would send `chars` once per box and every sort field, so the request
// URL is built here. Nothing here evaluates code: the CSP forbids it.
document.addEventListener('htmx:configRequest', (event) => {
  const form = event.detail.elt;
  if (!(form instanceof HTMLElement) || !form.hasAttribute('data-character-filter')) return;

  const query = [];
  const boxes = [...form.querySelectorAll('input[type="checkbox"][name="chars"]')];
  const selected = boxes.filter((box) => box.checked).map((box) => box.value);
  if (selected.length > 0 && selected.length < boxes.length) {
    query.push('chars=' + selected.map(encodeURIComponent).join(','));
  }
  const value = (name) => form.querySelector(`input[name="${name}"]`)?.value;
  for (const [sort, dir] of [['sort', 'dir'], ['bsort', 'bdir']]) {
    if (value(sort) && !(value(sort) === 'item' && value(dir) === 'asc')) {
      query.push(`${sort}=${encodeURIComponent(value(sort))}`, `${dir}=${encodeURIComponent(value(dir))}`);
    }
  }

  for (const key of Object.keys(event.detail.parameters)) delete event.detail.parameters[key];
  event.detail.path = query.length > 0 ? `/orders?${query.join('&')}` : '/orders';
});
