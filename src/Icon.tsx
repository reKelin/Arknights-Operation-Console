const paths = {
  settings:
    "M9 3 10 1h4l1 2 2 1 2-.2 2 3.4-1.2 1.8v2l1.2 1.8-2 3.4-2-.2-2 1-1 2h-4l-1-2-2-1-2 .2-2-3.4L4.2 11V9L3 7.2l2-3.4L7 4Z",
  minus: "M5 12h14",
  close: "m6 6 12 12M6 18 18 6",
  back: "m10 5-7 7 7 7M3 12h18",
  fit: "M12 3v3m0 12v3M3 12h5m8 0h5M5 9l3 3-3 3m14-6-3 3 3 3",
  zoomOut: "M15 15l6 6M5 9h8",
  zoomIn: "M15 15l6 6M5 9h8M9 5v8",
  plus: "M12 4v16M4 12h16",
  folder: "M3 7V4h6l2 3h10v13H3ZM3 10h18",
} as const;

export default function Icon({ name }: { name: keyof typeof paths }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={paths[name]} />
      {name === "settings" && <circle cx="12" cy="10" r="3" />}
      {(name === "zoomIn" || name === "zoomOut") && (
        <circle cx="9" cy="9" r="7" />
      )}
    </svg>
  );
}
