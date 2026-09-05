import { route, notFound, startRouter, navigate } from "./router";
import { state } from "./state";
import { renderLogin } from "./views/login-view";
import { renderEntryList } from "./views/entry-list-view";
import { renderPreview } from "./views/preview-view";
import { renderEditor } from "./views/editor-view";

const app = document.querySelector<HTMLDivElement>("#app")!;

function requireAuth(render: () => void): void {
  if (!state.authenticated) {
    navigate("/login");
    return;
  }
  render();
}

route("/login", () => renderLogin(app));
route("/entries", () => requireAuth(() => renderEntryList(app)));
route("/entries/:id", (params) => requireAuth(() => renderPreview(app, { id: params.id })));
route("/entries/:id/edit", (params) => requireAuth(() => renderEditor(app, { id: params.id })));

notFound(() => navigate(state.authenticated ? "/entries" : "/login"));

startRouter();
