export function Panel({ title }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header" />
    </section>
  );
}

export function App() {
  const [tag, setTag] = createSignal("in");
  return (
    <div>
      <Panel
        title="Status"
        header={<div><Slot name="inner"><em class="none">{tag}</em></Slot></div>}
        inner={<b class="hl">{tag}</b>}
      />
      <Panel
        title="Bare"
        header={<div><Slot name="inner"><em class="none">{tag}</em></Slot></div>}
      />
    </div>
  );
}
