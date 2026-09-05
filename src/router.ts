type RouteHandler = (params: Record<string, string>) => void;

interface Route {
  pattern: RegExp;
  keys: string[];
  handler: RouteHandler;
}

const routes: Route[] = [];
let notFoundHandler: RouteHandler = () => {};

function compile(path: string): { pattern: RegExp; keys: string[] } {
  const keys: string[] = [];
  const pattern = path
    .split("/")
    .map((segment) => {
      if (segment.startsWith(":")) {
        keys.push(segment.slice(1));
        return "([^/]+)";
      }
      return segment.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    })
    .join("/");
  return { pattern: new RegExp(`^${pattern}$`), keys };
}

export function route(path: string, handler: RouteHandler): void {
  const { pattern, keys } = compile(path);
  routes.push({ pattern, keys, handler });
}

export function notFound(handler: RouteHandler): void {
  notFoundHandler = handler;
}

export function navigate(path: string): void {
  window.location.hash = path;
}

function resolve(): void {
  const hash = window.location.hash.replace(/^#/, "") || "/login";
  for (const candidate of routes) {
    const match = candidate.pattern.exec(hash);
    if (match) {
      const params: Record<string, string> = {};
      candidate.keys.forEach((key, i) => {
        params[key] = decodeURIComponent(match[i + 1]);
      });
      candidate.handler(params);
      return;
    }
  }
  notFoundHandler({});
}

export function startRouter(): void {
  window.addEventListener("hashchange", resolve);
  resolve();
}
