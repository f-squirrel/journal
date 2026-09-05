import { navigate } from "../router";
import { state } from "../state";

export function renderLogin(container: HTMLElement): void {
  container.innerHTML = "";

  const wrapper = document.createElement("div");
  wrapper.className = "auth-screen";
  wrapper.innerHTML = `
    <div class="auth-card">
      <h1>Journal</h1>
      <p class="auth-subtitle">Sign in to continue</p>
      <form id="login-form" class="auth-form">
        <label for="email">Email</label>
        <input id="email" name="email" type="email" placeholder="you@example.com" required />
        <label for="password">Passphrase</label>
        <input id="password" name="password" type="password" placeholder="••••••••" required />
        <button type="submit">Sign in</button>
        <p class="auth-error" id="login-error" hidden>Enter an email and passphrase to continue.</p>
      </form>
    </div>
  `;
  container.appendChild(wrapper);

  const form = wrapper.querySelector<HTMLFormElement>("#login-form")!;
  const error = wrapper.querySelector<HTMLParagraphElement>("#login-error")!;

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const email = (form.elements.namedItem("email") as HTMLInputElement).value.trim();
    const password = (form.elements.namedItem("password") as HTMLInputElement).value;

    if (!email || !password) {
      error.hidden = false;
      return;
    }

    // TODO: replace with real authentication against the vault backend.
    state.authenticated = true;
    navigate("/entries");
  });
}
