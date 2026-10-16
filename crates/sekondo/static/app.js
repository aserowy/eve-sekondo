// htmx would inject an inline <style> for indicators, which the CSP forbids.
// The indicator styles live in app.css instead.
htmx.config.includeIndicatorStyles = false;
// No eval: the CSP forbids it, and nothing here needs hx-on or hx-vals javascript.
htmx.config.allowEval = false;
