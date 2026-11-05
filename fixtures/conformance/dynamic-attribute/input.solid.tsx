export function Avatar() {
  const [url, setUrl] = createSignal("a.png");
  return <img src={url()} alt="avatar" />;
}
