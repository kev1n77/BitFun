/** Page object for the shipped BitFun update and About dialogs. */
export class UpdatePage {
  constructor(execute, until) { this.execute = execute; this.until = until; }

  dialogText() {
    return this.execute(`
      const root = document.querySelector('[data-bf-component="update"][data-bf-part="availableRoot"]');
      return root && root.getBoundingClientRect().height > 0 && root.innerText.includes('OpenBitFun 1.0') ? root.innerText : null;`);
  }

  async checkFromAbout() {
    await this.click('[data-testid="nav-footer-more-btn"]');
    await this.until('Original About menu item', () => this.execute(`
      const button = Array.from(document.querySelectorAll('[data-testid="nav-footer-menu"] [role="menuitem"]')).at(-1);
      if (!button || !/关于|About/i.test(button.innerText)) return false;
      button.click(); return true;`));
    await this.click('[data-bf-component="about-dialog"][data-bf-part="updateActions"] button');
  }

  click(selector) {
    return this.until(`Original UI action ${selector}`, () => this.execute(`
      const button = document.querySelector(arguments[0]);
      if (!button || button.disabled || button.getBoundingClientRect().height === 0) return false;
      button.click(); return true;`, [selector]));
  }
}
