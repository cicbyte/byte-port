//! PortApp：主视图 Entity。
//!
//! 状态机：首页态（view_home）/ 结果态 × 端口/孤儿 tab。
//! 两个 tab 各自独立缓存快照，切 tab 不重扫；搜索才触发后台扫描。
//! 扫描走 tig 模式：cx.spawn + background_spawn + revision 防过期。

use byte_port_core::proc_tree::{build_subtree, kill_tree};
use byte_port_core::Pid;
use gpui_kit::base::{h_flex, v_flex, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{
    dialog::{DialogAction, DialogClose, DialogFooter},
    input::{Input, InputEvent, InputState},
    tab::{Tab as TabWidget, TabBar},
    table::{DataTable, TableState},
    ActiveTheme, Icon, IconName, Sizable, WindowExt,
};
// gpui_kit::* 即 gpui 全量重导出（Entity/Context/Window/div/px…）
use gpui_kit::*;

use crate::scan::{filter_orphans, filter_ports, flatten_tree, scan_snapshot, Snapshot};
use crate::tables::{orphan_rows, OrphansDelegate, PortsDelegate};

#[derive(Clone, Copy, PartialEq)]
pub enum ViewTab {
    Ports,
    Orphans,
}

pub struct PortApp {
    view_home: bool,
    pub tab: ViewTab,
    ports_snap: Option<Snapshot>,
    orphans_snap: Option<Snapshot>,
    home_input: Entity<InputState>,
    bar_input: Entity<InputState>,
    ports_table: Entity<TableState<PortsDelegate>>,
    orphans_table: Entity<TableState<OrphansDelegate>>,
    status: SharedString,
    revision: u64,
    _scan_task: Option<Task<()>>,
    _subs: Vec<gpui::Subscription>,
}

impl PortApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();

        let home_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("输入端口号 / PID / 进程名 / 路径…")
        });
        let bar_input = cx.new(|cx| InputState::new(window, cx).placeholder("过滤…"));

        let ports_table =
            cx.new(|cx| TableState::new(PortsDelegate::new(weak.clone()), window, cx));
        let orphans_table =
            cx.new(|cx| TableState::new(OrphansDelegate::new(weak.clone()), window, cx));

        // 回车即搜索（两个输入框都订阅）
        let home_sub = cx.subscribe_in(
            &home_input,
            window,
            |this, _s, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.on_search(window, cx);
                }
            },
        );
        let bar_sub = cx.subscribe_in(
            &bar_input,
            window,
            |this, _s, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.on_search(window, cx);
                }
            },
        );

        Self {
            view_home: true,
            tab: ViewTab::Ports,
            ports_snap: None,
            orphans_snap: None,
            home_input,
            bar_input,
            ports_table,
            orphans_table,
            status: "输入关键词，回车或点击搜索".into(),
            revision: 0,
            _scan_task: None,
            _subs: vec![home_sub, bar_sub],
        }
    }

    // ---------- 搜索与扫描 ----------

    /// 搜索入口：同步输入框 → 有缓存先即时过滤 → 进结果态 → 后台刷新。
    fn on_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keyword = if self.view_home {
            self.home_input.read(cx).value().to_string()
        } else {
            self.bar_input.read(cx).value().to_string()
        };
        // 同步另一个输入框（set_value 不触发 Change，无回环）
        if self.view_home {
            let src = keyword.clone();
            self.bar_input
                .update(cx, |s, cx| s.set_value(src, window, cx));
        } else {
            let src = keyword.clone();
            self.home_input
                .update(cx, |s, cx| s.set_value(src, window, cx));
        }
        // 有缓存先立即渲染（即时反馈），随后后台扫描刷新
        self.refresh_current_table(&keyword, cx);
        if self.ports_snap.is_none() && self.tab == ViewTab::Ports
            || self.orphans_snap.is_none() && self.tab == ViewTab::Orphans
        {
            self.status = "扫描中…".into();
        }
        self.view_home = false;
        cx.notify();
        self.start_scan(cx);
    }

    /// 后台扫描当前 tab（tig 模式：background_spawn + revision 防过期）。
    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        let revision = self.revision;
        self._scan_task = Some(cx.spawn(async move |this, cx| {
            let snap = cx.background_spawn(async move { scan_snapshot() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.revision != revision {
                    return; // 已有更新的扫描发起，丢弃过期结果
                }
                match this.tab {
                    ViewTab::Ports => this.ports_snap = Some(snap),
                    ViewTab::Orphans => this.orphans_snap = Some(snap),
                }
                this.refresh_current_table(&this.current_keyword(cx), cx);
                cx.notify();
            });
        }));
    }

    /// 当前生效的搜索词（按视图取对应输入框）。
    fn current_keyword(&self, cx: &App) -> String {
        if self.view_home {
            self.home_input.read(cx).value().to_string()
        } else {
            self.bar_input.read(cx).value().to_string()
        }
    }

    /// 按当前 tab 用缓存 + 关键词刷新表格。
    fn refresh_current_table(&mut self, keyword: &str, cx: &mut Context<Self>) {
        match self.tab {
            ViewTab::Ports => {
                let rows = self
                    .ports_snap
                    .as_ref()
                    .map(|s| filter_ports(s, keyword))
                    .unwrap_or_default();
                let total = self.ports_snap.as_ref().map(|s| s.ports.len()).unwrap_or(0);
                self.ports_table.update(cx, |t, cx| {
                    t.delegate_mut().rows = rows;
                    t.refresh(cx);
                });
                self.status = format!("共 {total} 条").into();
            }
            ViewTab::Orphans => {
                let display = self
                    .orphans_snap
                    .as_ref()
                    .map(|s| orphan_rows(&filter_orphans(s, keyword), &s.sys))
                    .unwrap_or_default();
                let total = self.orphans_snap.as_ref().map(|s| s.orphans.len()).unwrap_or(0);
                self.orphans_table.update(cx, |t, cx| {
                    t.delegate_mut().rows = display;
                    t.refresh(cx);
                });
                self.status = format!("共 {total} 个孤儿").into();
            }
        }
    }

    fn switch_tab(&mut self, tab: ViewTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        let kw = self.current_keyword(cx);
        self.refresh_current_table(&kw, cx);
        cx.notify();
    }

    // ---------- 行内操作（由表格 delegate 的按钮回调） ----------

    /// 端口页「结束」：子树预览弹窗。
    pub fn confirm_kill_port(&mut self, pid: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.ports_snap.as_ref() else {
            return;
        };
        let tree = build_subtree(&snap.sys, Pid::from_u32(pid));
        let total = tree.total_count();
        let mut nodes: Vec<(String, usize)> = Vec::new();
        flatten_tree(&tree, 0, &mut nodes);
        let weak = cx.entity().downgrade();

        window.open_dialog(cx, move |dialog, _w, _cx| {
            dialog
                .title(SharedString::from(format!(
                    "结束进程 PID {pid}（含子进程，共 {total} 个）"
                )))
                .child(
                    v_flex()
                        .gap_1()
                        .max_h(px(280.))
                        .children(nodes.iter().map(|(label, depth)| {
                            div()
                                .pl(px(*depth as f32 * 20.))
                                .text_sm()
                                .child(label.clone())
                        })),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel").outline().label("取消")),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("ok").danger().label("结束")),
                        ),
                )
                .on_ok({
                    let weak = weak.clone();
                    move |_, _w, cx| {
                    if let Some(app) = weak.upgrade() {
                        app.update(cx, |this, cx| this.do_kill_port(pid, cx));
                    }
                    true
                    }
                })
        });
    }

    /// 孤儿页「清理」：子树预览弹窗。
    pub fn confirm_clean_orphan(&mut self, pid: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.orphans_snap.as_ref() else {
            return;
        };
        let tree = build_subtree(&snap.sys, Pid::from_u32(pid));
        let total = tree.total_count();
        let mut nodes: Vec<(String, usize)> = Vec::new();
        flatten_tree(&tree, 0, &mut nodes);
        let weak = cx.entity().downgrade();

        window.open_dialog(cx, move |dialog, _w, _cx| {
            dialog
                .title(SharedString::from(format!(
                    "清理孤儿 PID {pid}（含子进程，共 {total} 个）"
                )))
                .child(
                    v_flex()
                        .gap_1()
                        .max_h(px(280.))
                        .children(nodes.iter().map(|(label, depth)| {
                            div()
                                .pl(px(*depth as f32 * 20.))
                                .text_sm()
                                .child(label.clone())
                        })),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel").outline().label("取消")),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("ok").danger().label("清理")),
                        ),
                )
                .on_ok({
                    let weak = weak.clone();
                    move |_, _w, cx| {
                    if let Some(app) = weak.upgrade() {
                        app.update(cx, |this, cx| this.do_clean_orphan(pid, cx));
                    }
                    true
                    }
                })
        });
    }

    /// 孤儿页「全部清理」：alert 确认。
    pub fn confirm_clean_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.orphans_snap.as_ref().map(|s| s.orphans.len()).unwrap_or(0);
        if count == 0 {
            self.status = "当前没有孤儿进程可清理".into();
            cx.notify();
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _w, _cx| {
            alert
                .title("全部清理孤儿")
                .description(format!("发现 {count} 个孤儿进程，即将全部清理（含各自子树）。"))
                .confirm()
                .ok_text("全部清理")
                .ok_variant(ButtonVariant::Danger)
                .on_ok({
                    let weak = weak.clone();
                    move |_, _w, cx| {
                    if let Some(app) = weak.upgrade() {
                        app.update(cx, |this, cx| this.do_clean_all(cx));
                    }
                    true
                    }
                })
        });
    }

    // ---------- 执行 kill ----------

    fn do_kill_port(&mut self, pid: u32, cx: &mut Context<Self>) {
        let (ok, fail) = match self.ports_snap.as_ref() {
            Some(snap) => {
                let tree = build_subtree(&snap.sys, Pid::from_u32(pid));
                kill_tree(&snap.sys, &tree)
                    .into_iter()
                    .fold((0, 0), |(o, f), (_, b)| if b { (o + 1, f) } else { (o, f + 1) })
            }
            None => (0, 0),
        };
        self.status = format!("PID {pid} 完成：成功 {ok} / 失败 {fail}").into();
        cx.notify();
        self.start_scan(cx);
    }

    fn do_clean_orphan(&mut self, pid: u32, cx: &mut Context<Self>) {
        let (ok, fail) = match self.orphans_snap.as_ref() {
            Some(snap) => {
                let tree = build_subtree(&snap.sys, Pid::from_u32(pid));
                kill_tree(&snap.sys, &tree)
                    .into_iter()
                    .fold((0, 0), |(o, f), (_, b)| if b { (o + 1, f) } else { (o, f + 1) })
            }
            None => (0, 0),
        };
        self.status = format!("孤儿 PID {pid} 完成：成功 {ok} / 失败 {fail}").into();
        cx.notify();
        self.start_scan(cx);
    }

    fn do_clean_all(&mut self, cx: &mut Context<Self>) {
        let (ok, fail) = match self.orphans_snap.as_ref() {
            Some(snap) => {
                let pids: Vec<Pid> = snap.orphans.iter().map(|o| o.pid).collect();
                let mut ok = 0;
                let mut fail = 0;
                for pid in pids {
                    let tree = build_subtree(&snap.sys, pid);
                    for (_, b) in kill_tree(&snap.sys, &tree) {
                        if b {
                            ok += 1;
                        } else {
                            fail += 1;
                        }
                    }
                }
                (ok, fail)
            }
            None => (0, 0),
        };
        self.status = format!("全部清理完成：成功 {ok} / 失败 {fail}").into();
        cx.notify();
        self.start_scan(cx);
    }
}

impl Render for PortApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view_home {
            self.render_home(cx).into_any_element()
        } else {
            self.render_result(cx).into_any_element()
        }
    }
}

impl PortApp {
    // ---------- 首页态 ----------
    fn render_home(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg, muted_fg) = {
            let t = cx.theme();
            (t.background, t.foreground, t.muted_foreground)
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(bg)
            .text_color(fg)
            .child(
                div()
                    .text_3xl()
                    .font_bold()
                    .child("Byte Port"),
            )
            .child(
                div()
                    .text_color(muted_fg)
                    .child("端口 · 进程 · 孤儿管理"),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_6()
                    .child(
                        Input::new(&self.home_input)
                            .w(px(460.))
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).small()),
                    )
                    .child(
                        Button::new("home-search")
                            .primary()
                            .label("搜索")
                            .on_click(cx.listener(|this, _, window, cx| this.on_search(window, cx))),
                    ),
            )
            .child(
                h_flex()
                    .pt_4()
                    .child(self.render_tabs(cx, "home-tabs")),
            )
    }

    // ---------- 结果态 ----------
    fn render_result(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg, border, muted_fg) = {
            let t = cx.theme();
            (t.background, t.foreground, t.border, t.muted_foreground)
        };
        let status = self.status.clone();

        // 顶栏
        let toolbar = h_flex()
            .w_full()
            .h(px(52.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                Button::new("go-home")
                    .outline()
                    .label("← 首页")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.view_home = true;
                        cx.notify();
                    })),
            )
            .child(
                Input::new(&self.bar_input)
                    .w(px(320.))
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).small()),
            )
            .child(
                Button::new("bar-search")
                    .primary()
                    .label("搜索")
                    .on_click(cx.listener(|this, _, window, cx| this.on_search(window, cx))),
            )
            .child(div().flex_1())
            .child(self.render_tabs(cx, "bar-tabs"))
            .child(
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(status),
            );

        // 内容区
        let content = match self.tab {
            ViewTab::Ports => DataTable::new(&self.ports_table)
                .stripe(true)
                .into_any_element(),
            ViewTab::Orphans => v_flex()
                .p_2()
                .gap_2()
                .size_full()
                .child(
                    h_flex().justify_end().child(
                        Button::new("clean-all")
                            .danger()
                            .label("全部清理")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_clean_all(window, cx)
                            })),
                    ),
                )
                .child(
                    DataTable::new(&self.orphans_table)
                        .stripe(true)
                        .into_any_element(),
                )
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(toolbar)
            .child(content)
    }

    // ---------- tab 条 ----------
    fn render_tabs(&mut self, cx: &mut Context<Self>, id: &'static str) -> impl IntoElement {
        let selected = match self.tab {
            ViewTab::Ports => 0,
            ViewTab::Orphans => 1,
        };
        TabBar::new(id)
            .selected_index(selected)
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                let tab = if *ix == 0 { ViewTab::Ports } else { ViewTab::Orphans };
                this.switch_tab(tab, cx);
            }))
            .child(TabWidget::new().label("端口"))
            .child(TabWidget::new().label("孤儿"))
    }
}
