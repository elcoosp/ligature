export function Panel({ title }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header" />
      <Slot name="foot">
        <i class="fb">{title}</i>
      </Slot>
    </section>
  );
}

export function App() {
  const [label, setLabel] = createSignal("live");
  return (
    <Panel
      title="Status"
      header={<button onClick={() => setLabel("next")}>{label}</button>}
    />
  );
}
