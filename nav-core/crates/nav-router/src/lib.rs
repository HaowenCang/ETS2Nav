// nav-router：路线规划（P2 计划 §54-82）。
// 模块：
//   snap —— SnapPoint / 起终点吸附（P2-07 §54-57）
//   cost —— EdgeCostProvider / Fastest / Shortest / Balanced（P2-08 §58-69）
//   search —— Dijkstra Oracle + A*（P2-09 §70-78）
//   alternatives —— 多策略路线（P2-10 §79-82）
pub mod alternatives;
pub mod cost;
pub mod search;
pub mod snap;
