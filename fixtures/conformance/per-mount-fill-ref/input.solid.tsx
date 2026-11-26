export function Panel({ title }) {
  return (
    <section>
      <h2>{title}</h2>
      <Slot name="header" />
    </section>
  );
}

export function App() {
  const [on, setOn] = createSignal(true);
  let mark;
  return (
    <Show when={on()}>
      <Panel title="x" header={<b ref={mark}>in</b>} />
    </Show>
  );
}
