export function Panel({ title, children }) {
  return (
    <section class="panel">
      <h1>{title}</h1>
      {children}
    </section>
  );
}

export function App() {
  return (
    <Panel title="Status">
      <p>all green</p>
    </Panel>
  );
}
