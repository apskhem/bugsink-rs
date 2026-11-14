/** Upstream's `{% if messages %}` list. */
export default function Messages({ messages, className }: { messages: string[]; className?: string }) {
  if (!messages.length) {
    return null;
  }
  return (
    <ul className={className}>
      {messages.map((message) => (
        <li
          key={message}
          className="bg-cyan-50 dark:bg-cyan-900 border-2 border-cyan-800 dark:border-cyan-400 p-4 rounded-lg"
        >
          {message}
        </li>
      ))}
    </ul>
  );
}
