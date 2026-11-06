export function Panel({ title, children }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header">
        <em>no header</em>
      </Slot>
      {children}
    </section>
  );
}

export function App() {
  return (
    <div>
      <Panel title="Status" header={<h1>all green</h1>}>
        <p>body text</p>
      </Panel>
      <Panel title="Bare">
        <p>other body</p>
      </Panel>
    </div>
  );
}
