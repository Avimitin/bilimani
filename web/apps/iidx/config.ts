import type { Policy } from '@bilimani/controller';
import type { BilibiliConfig } from '@bilimani/platform-bilibili';
export interface Config {
  proxy: Record<string, string>;
  platform: BilibiliConfig & { type: 'bilibili' };
  requests: Policy;
  display: { feedLimit: number; infoRotationSeconds: number; vfdEffects: boolean };
}
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
export function parseConfig(value: unknown, search = ''): Config {
  if (!record(value)) throw new Error('config.json 必须是 JSON 对象');
  const { platform, requests, display, proxy } = value;
  if (!record(platform) || platform.type !== 'bilibili' || typeof platform.enabled !== 'boolean' || typeof platform.relayUrl !== 'string') throw new Error('platform 需要 type、enabled 和 relayUrl');
  const params = new URLSearchParams(search);
  const authCode = params.get('authCode')?.trim() ?? '';
  if (params.getAll('authCode').length > 1 || authCode.length > 512 || /[\x00-\x1f\x7f]/.test(authCode)) throw new Error('URL 的 authCode 参数无效');
  if (platform.enabled && !authCode) throw new Error('请在浏览器源 URL 后添加 ?authCode=主播身份码');
  if (!record(proxy) || !Object.entries(proxy).every(([name,url]) => /^[a-zA-Z0-9_-]+$/.test(name) && typeof url === 'string' && /^https?:\/\//.test(url))) throw new Error('proxy 必须将代理名称映射到上游 URL');
  if (/^\.\/_proxy\/[a-zA-Z0-9_-]+$/.test(platform.relayUrl)) {
    if (!(platform.relayUrl.split('/').at(-1)! in proxy)) throw new Error('relayUrl 使用的代理未在 proxy 中配置');
  } else {
    let relay: URL;
    try { relay = new URL(platform.relayUrl); } catch { throw new Error('platform.relayUrl 不是有效 URL'); }
    if (relay.protocol !== 'https:' || relay.username || relay.password || relay.search || relay.hash) throw new Error('relayUrl 必须是本页代理路径或 HTTPS 地址');
  }
  if (!record(requests) || typeof requests.prefix !== 'string' || !requests.prefix.trim()) throw new Error('requests.prefix 必须是非空命令');
  const bounds: Record<string, [number, number]> = { candidateLimit: [1, 100], selectionTimeoutSeconds: [1, 3600], cooldownSeconds: [0, 86400], maxPendingUsers: [1, 1000] };
  for (const [key, [min, max]] of Object.entries(bounds)) {
    const n = requests[key];
    if (typeof n !== 'number' || !Number.isInteger(n) || n < min || n > max) throw new Error(`requests.${key} 必须是 ${min}–${max} 的整数`);
  }
  if (!record(display) || typeof display.feedLimit !== 'number' || !Number.isInteger(display.feedLimit) || display.feedLimit < 1 || display.feedLimit > 100) throw new Error('display.feedLimit 必须是 1–100 的整数');
  const infoRotationSeconds = display.infoRotationSeconds === undefined ? 10 : display.infoRotationSeconds;
  if (typeof infoRotationSeconds !== 'number' || !Number.isInteger(infoRotationSeconds) || infoRotationSeconds < 1 || infoRotationSeconds > 3600) throw new Error('display.infoRotationSeconds 必须是 1–3600 的整数');
  const vfdEffects = display.vfdEffects === undefined ? true : display.vfdEffects;
  if (typeof vfdEffects !== 'boolean') throw new Error('display.vfdEffects 必须是 true 或 false');
  return { proxy: proxy as Record<string,string>, platform: { type: 'bilibili', enabled: platform.enabled, authCode, relayUrl: platform.relayUrl.replace(/\/+$/, '') }, requests: { ...requests, prefix: requests.prefix.trim() } as unknown as Policy, display: { feedLimit: display.feedLimit, infoRotationSeconds, vfdEffects } };
}
export async function loadConfig(): Promise<Config> {
  const response = await fetch(new URL('./config.json', document.baseURI), { cache: 'no-store', signal: AbortSignal.timeout(4000) });
  if (!response.ok) throw new Error(`读取 config.json 失败（HTTP ${response.status}）。请检查页面目录中的 config.json 是否完整，缺失时可从发布包补回。`);
  let value: unknown;
  try { value = await response.json(); } catch { throw new Error('config.json 格式错误，请检查逗号和双引号'); }
  return parseConfig(value, location.search);
}
