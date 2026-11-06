import { createSignal } from "solid-js";

export function Signup() {
  const [email, setEmail] = createSignal("");
  return (
    <form class="signup">
      <input value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
      <p>email: {email()}</p>
    </form>
  );
}
