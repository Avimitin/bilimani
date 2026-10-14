// Only the --preview build includes this source; production uses BilibiliSource.
import type { ChatSource } from '@bilimani/controller';

export function createChatSource(): ChatSource {
  let timer: ReturnType<typeof setInterval> | undefined;
  const messages = [
    ['预览观众', '这个涂鸦框很有街头感！'],
    ['Dancer 02', 'Let’s go! 下一首继续！'],
    ['节奏玩家', '绿色喷漆箭头好看'],
    ['Old School', 'DANCE DANCE REVOLUTION'],
    ['舞池观众', '加油！这首的节奏太棒了'],
  ];
  return {
    start(emit) {
      if (timer) return;
      emit({ type: 'connection', connected: true, message: '本地预览 · 模拟弹幕' });
      emit({ type: 'room', room: { id: 'preview', name: 'DANCE CREW', title: '今晚的舞池，交给你们。' } });
      let index = 0;
      const next = () => {
        const [name, text] = messages[index++ % messages.length]!;
        emit({ type: 'chat', chat: { user: `preview-${name}`, name: name!, text: text! } });
      };
      next();
      timer = setInterval(next, 2500);
    },
    stop() { clearInterval(timer); timer = undefined; },
  };
}
