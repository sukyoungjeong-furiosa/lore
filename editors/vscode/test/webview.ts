import { Browser, CDPSession } from 'playwright-core';
// Electron webviews expose guest CDP targets rather than workbench frames.
export class Webview {
  private sequence = 0;
  private contexts = new Set<number>();
  private pending = new Map<number, { resolve(value: any): void; reject(error: Error): void }>();
  private context?: number;
  private constructor(private cdp: CDPSession, private session: string) {
    cdp.on('Target.receivedMessageFromTarget', event => {
      if (event.sessionId !== session) return;
      const message = JSON.parse(event.message);
      if (message.method === 'Runtime.executionContextCreated') this.contexts.add(message.params.context.id);
      if (message.method === 'Runtime.executionContextDestroyed') this.contexts.delete(message.params.executionContextId);
      if (message.id) {
        const waiter = this.pending.get(message.id); this.pending.delete(message.id);
        if (message.error) waiter?.reject(new Error(message.error.message)); else waiter?.resolve(message.result);
      }
    });
  }
  private async send(method: string, params: object = {}): Promise<any> {
    const id = ++this.sequence;
    const result = new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
    await this.cdp.send('Target.sendMessageToTarget', { sessionId: this.session, message: JSON.stringify({ id, method, params }) });
    return result;
  }
  static async connect(browser: Browser, selector = '#title'): Promise<Webview> {
    const cdp = await browser.newBrowserCDPSession();
    const { targetInfos } = await cdp.send('Target.getTargets');
    const targets = targetInfos.filter(t => t.type === 'iframe' && t.url.includes('extensionId=lore-local.lore'));
    if (!targets.length) throw new Error('Lore webview target is unavailable');
    for (const target of targets) {
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId: target.targetId, flatten: false });
    const view = new Webview(cdp, sessionId); await view.send('Runtime.enable');
    for (let i = 0; i < 100; i++) {
      for (const contextId of view.contexts) {
        const response = await view.send('Runtime.evaluate', { expression: `!!document.querySelector(${JSON.stringify(selector)})`, contextId, returnByValue: true }).catch(() => undefined);
        if (response?.result.value) { view.context = contextId; return view; }
      }
      if (i > 3) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    }
    throw new Error('Lore webview DOM did not load');
  }
  async evaluate<T>(expression: string): Promise<T> {
    const response = await this.send('Runtime.evaluate', { expression, contextId: this.context, returnByValue: true });
    if (response.exceptionDetails) throw new Error(response.exceptionDetails.text);
    return response.result.value;
  }
  locator(selector: string) {
    const selected = `document.querySelector(${JSON.stringify(selector)})`;
    return {
      textContent: () => this.evaluate<string>(`${selected}?.textContent`),
      inputValue: () => this.evaluate<string>(`${selected}?.value`),
      count: () => this.evaluate<number>(`document.querySelectorAll(${JSON.stringify(selector)}).length`),
      isVisible: () => this.evaluate<boolean>(`!!${selected}?.getClientRects().length`),
      click: () => this.evaluate(`(() => { const e = ${selected}; if (!e || e.disabled || !e.getClientRects().length) throw new Error('Element unavailable: ' + ${JSON.stringify(selector)}); e.click(); })()`),
      fill: (value: string) => this.evaluate(`(() => { const e = ${selected}; e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); })()`),
    };
  }
}
