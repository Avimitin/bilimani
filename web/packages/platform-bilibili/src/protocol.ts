import { Unzlib } from 'fflate';
import type { Chat } from '@bilimani/controller';
const MAX = 4 * 1024 * 1024;
export class ProtocolError extends Error {
  constructor(readonly kind: 'size' | 'header' | 'compression' | 'json') { super(`弹幕报文错误：${kind}`); }
}
export type Packet = { type: 'auth'; code: number } | { type: 'heartbeat' } | { type: 'command'; value: unknown };
export function packet(op: number, body: string): Uint8Array<ArrayBuffer> {
  const data = new TextEncoder().encode(body);
  const output = new Uint8Array(16 + data.length);
  const view = new DataView(output.buffer);
  view.setUint32(0, output.length); view.setUint16(4, 16); view.setUint16(6, 1); view.setUint32(8, op); view.setUint32(12, 1);
  output.set(data, 16);
  return output;
}
export function decode(data: Uint8Array): Packet[] {
  let budget = MAX;
  const result: Packet[] = [];
  function inner(bytes: Uint8Array, depth: number) {
    if (bytes.length > MAX || depth > 4) throw new ProtocolError('size');
    for (let offset = 0; offset < bytes.length;) {
      if (bytes.length - offset < 16) throw new ProtocolError('header');
      const view = new DataView(bytes.buffer, bytes.byteOffset + offset, bytes.length - offset);
      const length = view.getUint32(0), header = view.getUint16(4), version = view.getUint16(6), op = view.getUint32(8);
      if (header < 16 || length < header || length > bytes.length - offset) throw new ProtocolError('header');
      const body = bytes.subarray(offset + header, offset + length);
      if (result.length >= 4096) throw new ProtocolError('size');
      if (op === 5 && version === 2) {
        const chunks: Uint8Array[] = [];
        let size = 0;
        const inflater = new Unzlib(chunk => { size += chunk.length; if (size > budget) throw new ProtocolError('size'); chunks.push(chunk); });
        inflater.push(body, true);
        budget -= size;
        const expanded = new Uint8Array(size);
        let at = 0;
        for (const chunk of chunks) { expanded.set(chunk, at); at += chunk.length; }
        inner(expanded, depth + 1);
      } else if (op === 5 && version === 3) throw new ProtocolError('compression');
      else if ((op === 5 || op === 8) && body.length) {
        let value;
        try { value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(body)); } catch { throw new ProtocolError('json'); }
        if (op === 8) result.push({ type: 'auth', code: typeof value.code === 'number' ? value.code : -1 });
        else result.push({ type: 'command', value });
      } else if (op === 3) {
        result.push({ type: 'heartbeat' });
        if (new TextDecoder().decode(bytes.subarray(offset + length)) === '{}') return;
      }
      offset += length;
    }
  }
  inner(data, 0);
  return result;
}
export function chatFromCommand(value: unknown): { chat: Chat; id: string } | null {
  if (!value || typeof value !== 'object') return null;
  const message = value as { cmd?: unknown; data?: Record<string, unknown> };
  if (typeof message.cmd !== 'string' || message.cmd.split(':')[0] !== 'LIVE_OPEN_PLATFORM_DM') return null;
  const data = message.data;
  if (!data || data.is_mirror === true || typeof data.msg !== 'string' || data.msg.length > 1024 || typeof data.uname !== 'string') return null;
  const user = typeof data.open_id === 'string' && data.open_id ? `open:${data.open_id}` :
    typeof data.uid === 'number' && data.uid > 0 ? `uid:${data.uid}` : null;
  return user ? { chat: { user, name: data.uname, text: data.msg }, id: typeof data.msg_id === 'string' ? data.msg_id : '' } : null;
}
