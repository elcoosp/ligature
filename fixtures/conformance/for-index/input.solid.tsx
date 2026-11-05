export function Rows() {
  const [rows] = createSignal(["alpha", "beta"]);
  return (
    <ul>
      <For each={rows()}>{(r, i) => <li>{i()} {r}</li>}</For>
    </ul>
  );
}
