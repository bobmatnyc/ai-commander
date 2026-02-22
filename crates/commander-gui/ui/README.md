# AI Commander GUI - Svelte Frontend

Modern Svelte + TypeScript UI for the AI Commander desktop application.

## Tech Stack

- **Svelte 4**: Component framework
- **TypeScript**: Type-safe development
- **Tailwind CSS**: Utility-first styling
- **Vite**: Fast build tool
- **Tauri API**: Native desktop integration
- **lucide-svelte**: Icon library

## Project Structure

```
ui/
├── src/
│   ├── lib/
│   │   ├── components/        # Svelte components
│   │   │   ├── SessionList.svelte
│   │   │   ├── ChatView.svelte
│   │   │   ├── InputArea.svelte
│   │   │   └── BotStatus.svelte
│   │   └── stores/            # Svelte stores
│   │       └── app.ts         # Global app state
│   ├── App.svelte             # Root component
│   ├── main.ts                # Entry point
│   └── app.css                # Global styles
├── index.html                 # HTML template
└── vite.config.ts             # Vite configuration
```

## Development

### Install Dependencies

```bash
npm install
```

### Development Server (UI only)

```bash
npm run dev
```

This starts Vite dev server on http://localhost:5173

### Full Desktop App (Tauri + UI)

```bash
# From crates/commander-gui/
cargo tauri dev
```

This builds both Rust backend and Svelte frontend.

### Production Build

```bash
npm run build
```

Outputs to `dist/` directory.

## Components

### SessionList.svelte

- Lists all available Telegram sessions
- Auto-refreshes every 2 seconds
- Shows connection status with indicator
- Click to connect to session

### ChatView.svelte

- Displays chat messages (sent/received/system)
- Auto-scrolls to bottom on new messages
- Manual scroll with return-to-bottom button
- Listens for `session-output` events from Tauri

### InputArea.svelte

- Text input for sending messages
- Enter to send (Shift+Enter for newline)
- Disabled when no session connected
- Integrates with `send_message` IPC command

### BotStatus.svelte

- Shows bot running status (running/stopped)
- Displays bot PID when running
- Start/Stop buttons
- Auto-refreshes status every 5 seconds

## Svelte Stores

The app uses Svelte stores for global state management:

```typescript
// src/lib/stores/app.ts
export const sessions = writable<Session[]>([]);
export const currentSession = writable<Session | null>(null);
export const messages = writable<Message[]>([]);
export const botRunning = writable(false);
export const botPid = writable<number | null>(null);
```

## Tauri IPC Commands Used

The UI integrates with these Tauri backend commands:

- `list_sessions()` - Get all Telegram sessions
- `connect_session(name)` - Connect to specific session
- `disconnect_session()` - Disconnect current session
- `send_message(content)` - Send message to current session
- `get_bot_status()` - Check if bot is running
- `start_bot()` - Start the Telegram bot daemon
- `stop_bot()` - Stop the Telegram bot daemon
- `get_bot_logs()` - Retrieve bot logs (if implemented)

## Events

The UI listens for Tauri events:

- `session-output` - Received messages from Telegram session

## Styling

Uses Tailwind CSS with custom component styles. Key patterns:

- Flexbox layouts for responsive design
- Utility classes for spacing/colors
- Component-scoped styles in `<style>` blocks
- CSS custom properties for theme consistency

## TypeScript Types

All components use TypeScript with defined interfaces:

```typescript
interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
}

interface Message {
  direction: 'sent' | 'received' | 'system';
  content: string;
  timestamp: Date;
}

interface BotStatus {
  running: boolean;
  pid: number | null;
}
```

## Browser Compatibility

Built for modern desktop environments via Tauri:

- ES2021+ JavaScript
- Chrome 100+
- Safari 13+

## Performance

- Vite HMR for instant dev updates
- Optimized production builds with tree-shaking
- Efficient reactivity via Svelte compiler
- Minimal bundle size (~24KB JS gzipped)

## Next Steps

- Add error handling UI components
- Implement settings/configuration panel
- Add message search/filtering
- Enhance accessibility (ARIA labels, keyboard nav)
- Add dark mode support
