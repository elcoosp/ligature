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
  const [tag, setTag] = createSignal("in");
  let mark;
  return (
    <Show when={on()}>
      <Panel
        title="x"
        header={<div><Slot name="inner"><em class="none">{tag}</em></Slot></div>}
        inner={<b ref={mark}>{tag}</b>}
      />
    </Show>
  );
}
