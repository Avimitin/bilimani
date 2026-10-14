import type { ChatSource } from '@bilimani/controller';
import { BilibiliSource, type BilibiliConfig, type RoomTitleState } from '@bilimani/platform-bilibili';
import type { Config } from './config';

// OBS may recreate the chat connection when showing the source. Only a page
// reload resets the title request; this state is never stored outside the page.
const roomTitle: RoomTitleState = { requested: false, value: null };
export function createChatSource(platform: BilibiliConfig, _requests: Config['requests']): ChatSource {
  return new BilibiliSource(platform, roomTitle);
}
