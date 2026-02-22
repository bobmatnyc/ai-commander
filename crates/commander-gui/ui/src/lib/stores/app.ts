import { writable } from 'svelte/store';

export interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
}

export interface Message {
  direction: 'sent' | 'received' | 'system';
  content: string;
  timestamp: Date;
}

export interface BotStatus {
  running: boolean;
  pid: number | null;
}

export const sessions = writable<Session[]>([]);
export const currentSession = writable<Session | null>(null);
export const messages = writable<Message[]>([]);
export const botRunning = writable(false);
export const botPid = writable<number | null>(null);
