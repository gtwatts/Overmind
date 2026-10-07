// Overmind entry point for the bundled Cursor helper.
//
// Overmind spawns this script with stdin piped and never writes to it. When the
// Overmind process exits (normally or not) the pipe closes, and the helper exits
// with it, so no orphaned Node process is left behind.
process.stdin.on("end", () => process.exit(0));
process.stdin.on("error", () => process.exit(0));
process.stdin.resume();

await import("./dist/index.js");
