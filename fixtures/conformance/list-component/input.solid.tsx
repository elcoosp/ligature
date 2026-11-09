import { For } from "solid-js";

export function App() {
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  return (
    <main>
      <For each={rows()} key={(t) => t}>{(t) => <Badge count={t} />}</For>
    </main>
  );
}

export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}
