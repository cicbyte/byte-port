//! 端口 / 孤儿表格 delegate。
//!
//! gpui-kit 的 DataTable 不传 Model，而是实现 TableDelegate：
//! delegate 自持 `Vec<T>` 行数据，更新时 `table.update(cx, |t, cx| {
//! t.delegate_mut().rows = ...; t.refresh(cx); })`。
//! 操作列在 render_td 里返回 Button，点击回调经 WeakEntity<PortApp>
//! 回到主视图开确认弹窗。

use byte_port_core::model::PortEntry;
use byte_port_core::proc_tree::ProcNode;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::Sizable;
use gpui_kit::*;

use crate::app::PortApp;
use crate::scan::{proto_label, orphan_children};
use byte_port_core::System;

/// 孤儿行的展示数据（子进程数在扫描时预计算，避免 render 反复查 sys）。
pub struct OrphanDisplay {
    pub pid: u32,
    pub name: String,
    pub path: String,
    pub children: usize,
}

fn text_cell(s: &str) -> AnyElement {
    div_str(s).into_any_element()
}

/// 单元格文本 div（行高内垂直居中）。
fn div_str(s: &str) -> impl IntoElement {
    div().flex().items_center().child(s.to_string())
}

// ============ 端口表 ============

pub struct PortsDelegate {
    pub rows: Vec<PortEntry>,
    columns: Vec<Column>,
    app: WeakEntity<PortApp>,
}

impl PortsDelegate {
    pub fn new(app: WeakEntity<PortApp>) -> Self {
        Self {
            rows: Vec::new(),
            columns: vec![
                Column::new("proto", "协议").width(px(64.)),
                Column::new("addr", "本地地址:端口").width(px(170.)),
                Column::new("state", "状态").width(px(110.)),
                Column::new("pid", "PID").width(px(76.)),
                Column::new("name", "进程名").width(px(150.)),
                Column::new("path", "路径").width(px(340.)),
                Column::new("op", "操作").width(px(90.)),
            ],
            app,
        }
    }
}

impl TableDelegate for PortsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix) else {
            return text_cell("");
        };
        match col_ix {
            0 => text_cell(proto_label(&row.protocol)),
            1 => text_cell(&row.local_addr),
            2 => text_cell(&row.state),
            3 => text_cell(&row.pid.map(|p| p.to_string()).unwrap_or_default()),
            4 => text_cell(row.process_name.as_deref().unwrap_or("")),
            5 => text_cell(
                &row
                    .exe_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            6 => {
                // 操作列：有 PID 才显示「结束」
                match row.pid {
                    Some(pid) => {
                        let weak = self.app.clone();
                        Button::new(SharedString::from(format!("kill-{row_ix}")))
                            .label("结束")
                            .small()
                            .danger()
                            .on_click(move |_, window, cx| {
                                if let Some(app) = weak.upgrade() {
                                    app.update(cx, |this, cx| {
                                        this.confirm_kill_port(pid, window, cx)
                                    });
                                }
                            })
                            .into_any_element()
                    }
                    None => text_cell(""),
                }
            }
            _ => text_cell(""),
        }
    }
}

// ============ 孤儿表 ============

pub struct OrphansDelegate {
    pub rows: Vec<OrphanDisplay>,
    columns: Vec<Column>,
    app: WeakEntity<PortApp>,
}

impl OrphansDelegate {
    pub fn new(app: WeakEntity<PortApp>) -> Self {
        Self {
            rows: Vec::new(),
            columns: vec![
                Column::new("pid", "PID").width(px(80.)),
                Column::new("name", "进程名").width(px(160.)),
                Column::new("path", "路径").width(px(430.)),
                Column::new("children", "子进程数").width(px(90.)),
                Column::new("op", "操作").width(px(90.)),
            ],
            app,
        }
    }
}

impl TableDelegate for OrphansDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix) else {
            return text_cell("");
        };
        match col_ix {
            0 => text_cell(&row.pid.to_string()),
            1 => text_cell(&row.name),
            2 => text_cell(&row.path),
            3 => text_cell(&row.children.to_string()),
            4 => {
                let weak = self.app.clone();
                let pid = row.pid;
                Button::new(SharedString::from(format!("clean-{row_ix}")))
                    .label("清理")
                    .small()
                    .danger()
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak.upgrade() {
                            app.update(cx, |this, cx| {
                                this.confirm_clean_orphan(pid, window, cx)
                            });
                        }
                    })
                    .into_any_element()
            }
            _ => text_cell(""),
        }
    }
}

/// 从孤儿快照构建展示行（预计算子进程数）。
pub fn orphan_rows(orphans: &[ProcNode], sys: &System) -> Vec<OrphanDisplay> {
    orphans
        .iter()
        .map(|o| OrphanDisplay {
            pid: o.pid.as_u32(),
            name: o.name.clone(),
            path: o
                .exe
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            children: orphan_children(sys, &o.pid),
        })
        .collect()
}
