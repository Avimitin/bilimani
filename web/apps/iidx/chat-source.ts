import type { ChatSource } from '@bilimani/controller';
import { BilibiliSource, type BilibiliConfig } from '@bilimani/platform-bilibili';
import type { Config } from './config';

export function createChatSource(platform: BilibiliConfig, _requests: Config['requests']): ChatSource {
  return new BilibiliSource(platform);
}
