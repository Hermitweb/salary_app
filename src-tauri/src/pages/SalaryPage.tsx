// 工资条页：与 Python 版 templates/salary.html 保持一致。

import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import {
  AuthTokens,
  auditExport,
  getAvailableMonths,
  getSalary,
  logout,
  type MonthOption,
} from '../services/api';

interface PayslipField {
  key: string;
  label: string;
  value: string;
  isNet: boolean;
}

/// 实发工资列的候选名称。
const NET_SALARY_KEYS = ['本月实发工资', '实发工资'];

/** HTML 转义：导出窗口用 document.write 写入完整 HTML 文档，动态文本必须先转义。 */
function escapeHtml(value: string): string {
  return String(value).replace(/[&<>"']/g, (char) => {
    switch (char) {
      case '&':
        return '&amp;';
      case '<':
        return '&lt;';
      case '>':
        return '&gt;';
      case '"':
        return '&quot;';
      default:
        return '&#39;';
    }
  });
}

export function SalaryPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const tableRef = useRef<HTMLTableElement | null>(null);

  const employee = AuthTokens.getUser();

  const [months, setMonths] = useState<MonthOption[]>([]);
  const [year, setYear] = useState(searchParams.get('year') ?? '');
  const [month, setMonth] = useState(searchParams.get('month') ?? '');
  const [fields, setFields] = useState<PayslipField[]>([]);
  const [netSalary, setNetSalary] = useState('0');
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState('');
  const [loading, setLoading] = useState(true);

  // 可选年份（倒序）与某一年份下的月份（倒序）
  const availableYears = Array.from(new Set(months.map((m) => m.year))).sort((a, b) => b - a);
  const monthsForYear = months
    .filter((m) => String(m.year) === year)
    .map((m) => m.month)
    .sort((a, b) => b - a);

  // 载入可用月份，并校正默认的年月
  useEffect(() => {
    let cancelled = false;

    getAvailableMonths().then((result) => {
      if (cancelled) return;

      const available = result.ok ? result.months : [];
      setMonths(available);

      const years = Array.from(new Set(available.map((m) => m.year))).sort((a, b) => b - a);
      if (years.length === 0) {
        setError(result.error || '没有可用的工资数据');
        setLoading(false);
        return;
      }

      const currentYear = Number(year);
      const nextYear = years.includes(currentYear) ? currentYear : years[0];

      const yearMonths = available
        .filter((m) => m.year === nextYear)
        .map((m) => m.month)
        .sort((a, b) => b - a);

      const currentMonth = Number(month);
      const nextMonth = yearMonths.includes(currentMonth) ? currentMonth : yearMonths[0];

      setYear(String(nextYear));
      setMonth(String(nextMonth));
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 查询工资条
  useEffect(() => {
    if (!year || !month) return;

    let cancelled = false;
    setLoading(true);
    setError(null);

    getSalary(Number(year), Number(month)).then((result) => {
      if (cancelled) return;

      if (result.ok && result.headers && result.values) {
        const nextFields: PayslipField[] = [];
        let net = '0';

        result.headers.forEach((header, index) => {
          const label = header.trim();
          if (!label || label === '员工号') return;

          const value = result.values?.[index] ?? '';
          const isNet = NET_SALARY_KEYS.includes(label);
          if (isNet) {
            net = value || '0';
          }

          nextFields.push({ key: label, label, value, isNet });
        });

        setFields(nextFields);
        setNetSalary(net);
        // 后端查不到该月份时会回落到最新的一份，并说明实际给的是哪个月
        setNotice(result.notice || '');
        if (result.year && result.month) {
          setYear(String(result.year));
          setMonth(String(result.month));
        }
      } else {
        setError(result.error || '查询失败');
        setFields([]);
        setNetSalary('0');
        setNotice('');
      }

      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [year, month]);

  // 把选中的年月同步到地址栏
  useEffect(() => {
    if (!year || !month) return;
    setSearchParams({ year, month }, { replace: true });
  }, [year, month, setSearchParams]);

  const handleYearChange = (value: string) => {
    setYear(value);
    // 换年后该年可能没有当前月份，退回到该年最新月份
    const yearMonths = months
      .filter((m) => String(m.year) === value)
      .map((m) => m.month)
      .sort((a, b) => b - a);
    setMonth(yearMonths.length > 0 ? String(yearMonths[0]) : '');
  };

  const handleQuery = () => {
    if (!year || !month) return;
    navigate(`/salary?year=${year}&month=${month}`);
  };

  const handleLogout = async () => {
    await logout();
    navigate('/login', { replace: true });
  };

  const titleText = `${year}年${Number(month)}月 工资条`;

  /** 打印当前工资条。 */
  const printSalary = () => {
    void auditExport('PRINT', `${year}-${month}`);
    window.print();
  };

  /** 导出 PDF：新开窗口写入打印专用文档并自动触发打印。 */
  const exportSalary = () => {
    void auditExport('EXPORT_PDF', `${year}-${month} 工资条PDF`);

    const win = window.open('', '_blank');
    if (!win) {
      alert('浏览器阻止了弹窗，请允许弹窗后重试');
      return;
    }

    const name = employee?.name ?? '';
    const title = `${name} - ${year}年${month}月工资条`;
    const tableEl = tableRef.current?.outerHTML ?? '';

    /* eslint-disable no-useless-escape --
       模板里的 <\/script> 必须保持转义：这段字符串最终会被写进导出 PDF 的
       HTML 文档，未转义的 </script> 会提前闭合内联脚本标签。 */
    win.document.write(`
      <!DOCTYPE html>
      <html lang="zh-CN">
      <head>
        <meta charset="utf-8">
        <title>${escapeHtml(title)}</title>
        <style>
          * { box-sizing: border-box; margin: 0; padding: 0; }
          @page { size: A4 landscape; margin: 8mm; }
          body {
            font-family: "Microsoft YaHei", "SimSun", sans-serif;
            background: #fff;
            color: #1a2332;
            padding: 16px;
          }
          .doc-title {
            text-align: center;
            font-size: 18px;
            font-weight: 700;
            margin-bottom: 8px;
          }
          .doc-subtitle {
            text-align: center;
            font-size: 14px;
            font-weight: 600;
            margin-bottom: 16px;
          }
          .payslip-table {
            border-collapse: collapse;
            font-size: 11px;
            background: #fff;
            min-width: max-content;
            width: auto;
          }
          .payslip-table th,
          .payslip-table td {
            border: 1px solid #1a2332;
            padding: 4px 3px;
            text-align: center;
            vertical-align: middle;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
          }
          .payslip-table thead th {
            background: #f8fafc;
            font-weight: 600;
          }
          .payslip-table .col-net {
            background: #fffbeb;
            font-weight: 700;
            color: #b45309;
          }
          .warn-msg {
            margin-top: 12px;
            padding: 6px 16px;
            background: #fef2f2;
            border: 1px dashed #fecaca;
            border-radius: 4px;
            color: #b91c1c;
            font-size: 12px;
            text-align: center;
            letter-spacing: 1px;
          }
          @media print {
            @page { size: A4 landscape; margin: 8mm; }
            body { padding: 0; }
          }
        </style>
      </head>
      <body>
        <div class="doc-title">员工工资条</div>
        <div class="doc-subtitle">${escapeHtml(titleText)}</div>
        ${tableEl}
        <div class="warn-msg">⚠ 薪资保密，请勿讨论，违者按公司制度处理</div>
        <script>
          window.onload = function() {
            setTimeout(function() {
              window.print();
              setTimeout(function() { window.close(); }, 300);
            }, 300);
          };
        <\/script>
      </body>
      </html>
    `);
    /* eslint-enable no-useless-escape */
    win.document.close();
  };

  /** 导出截图：把标题、表格和提示条渲染到临时容器后截图。 */
  const exportScreenshot = async () => {
    const table = tableRef.current;
    if (!table) return;

    void auditExport('EXPORT_IMG', `${year}-${month} 工资条截图`);

    const name = employee?.name ?? '';

    // 创建临时容器，按内容撑开，避免固定大宽度导致画布超限
    const temp = document.createElement('div');
    temp.style.position = 'fixed';
    temp.style.top = '0';
    temp.style.left = '0';
    temp.style.zIndex = '-9999';
    temp.style.background = '#ffffff';
    temp.style.padding = '24px';
    temp.style.width = 'max-content';
    temp.style.boxSizing = 'border-box';

    const titleEl = document.createElement('div');
    titleEl.textContent = titleText;
    titleEl.style.fontSize = '15px';
    titleEl.style.fontWeight = '600';
    titleEl.style.color = '#1a2332';
    titleEl.style.marginBottom = '16px';

    const cloneTable = table.cloneNode(true) as HTMLTableElement;
    cloneTable.style.width = 'max-content';
    cloneTable.style.minWidth = 'max-content';
    cloneTable.style.tableLayout = 'auto';

    temp.appendChild(titleEl);
    temp.appendChild(cloneTable);

    const warn = document.createElement('div');
    warn.className = 'warn-msg';
    warn.textContent = '⚠ 薪资保密，请勿讨论，违者按公司制度处理';
    warn.style.marginTop = '16px';
    temp.appendChild(warn);

    document.body.appendChild(temp);

    try {
      const html2canvas = (await import('html2canvas')).default;

      // 等浏览器完成一次布局。
      // 后台标签页里 requestAnimationFrame 可能不触发，所以同时兜一个定时器，
      // 否则截图会一直卡在这里。
      await new Promise<void>((resolve) => {
        let settled = false;
        const finish = () => {
          if (!settled) {
            settled = true;
            resolve();
          }
        };
        requestAnimationFrame(finish);
        window.setTimeout(finish, 200);
      });

      const rect = temp.getBoundingClientRect();
      // 额外留出 40px 边距，避免右侧/底部被截断
      const targetWidth = Math.ceil(rect.width) + 40;
      const targetHeight = Math.ceil(rect.height) + 40;

      // 避免画布超出浏览器限制导致 PNG 损坏
      let scale = 2;
      const maxCanvas = 16000;
      if (targetWidth * scale > maxCanvas || targetHeight * scale > maxCanvas) {
        scale = Math.min(maxCanvas / targetWidth, maxCanvas / targetHeight, 1);
      }

      const canvas = await html2canvas(temp, {
        scale,
        useCORS: true,
        backgroundColor: '#ffffff',
        logging: false,
        width: targetWidth,
        height: targetHeight,
        windowWidth: targetWidth,
        windowHeight: targetHeight,
        scrollX: 0,
        scrollY: 0,
      });

      const link = document.createElement('a');
      link.download = `${name}-${year}年${month}月工资条.png`;
      link.href = canvas.toDataURL('image/png');
      link.click();
    } catch (error) {
      alert('截图生成失败：' + (error as Error).message);
    } finally {
      if (temp.parentNode) {
        document.body.removeChild(temp);
      }
    }
  };

  // Ctrl/Cmd + E 导出 PDF
  // 用 ref 持有最新的导出函数，避免每次渲染都重新注册监听器
  const exportSalaryRef = useRef(exportSalary);
  exportSalaryRef.current = exportSalary;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === 'e') {
        event.preventDefault();
        if (fields.length > 0) {
          exportSalaryRef.current();
        }
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [fields.length]);

  return (
    <>
      <div className="header no-print">
        <h1>员工工资查询系统</h1>
        <div className="user-info">
          <span>{employee?.name ?? ''}</span>
          <button type="button" className="logout-btn" onClick={handleLogout}>
            退出
          </button>
        </div>
      </div>

      <div className="container">
        <div className="card selector-card no-print">
          <span className="selector-label">选择月份</span>
          <div className="selector-form">
            <select
              className="month-select"
              value={year}
              onChange={(e) => handleYearChange(e.target.value)}
              aria-label="年份"
            >
              {availableYears.map((y) => (
                <option key={y} value={y}>
                  {y}年
                </option>
              ))}
            </select>

            <select
              className="month-select"
              value={month}
              onChange={(e) => setMonth(e.target.value)}
              aria-label="月份"
            >
              {monthsForYear.map((m) => (
                <option key={m} value={m}>
                  {m}月
                </option>
              ))}
            </select>

            <button type="button" className="query-btn" onClick={handleQuery}>
              查询
            </button>
          </div>

          {fields.length > 0 && (
            <div className="salary-badge-wrapper no-print">
              <div className="salary-badge">
                <span className="salary-badge-label">实发工资</span>
                <span className="salary-badge-value">
                  {netSalary}
                  <span className="currency">￥</span>
                </span>
              </div>
            </div>
          )}
        </div>

        {notice && !error ? <div className="notice-card">{notice}</div> : null}

        {error ? (
          <div className="error-card">{error}</div>
        ) : loading ? (
          <div className="card">
            <div className="empty-hint">正在加载…</div>
          </div>
        ) : fields.length > 0 ? (
          <div className="card payslip-card">
            <div className="toolbar no-print">
              <div className="title-text">{titleText}</div>
              <div className="btn-group">
                <button type="button" className="print-btn" onClick={printSalary}>
                  🖨 打印
                </button>
                <button type="button" className="export-btn" onClick={exportSalary}>
                  ⬇ 导出PDF
                </button>
                <button type="button" className="export-img-btn" onClick={exportScreenshot}>
                  🖼 导出截图
                </button>
              </div>
            </div>

            <div className="payslip-wrapper" id="payslip-content">
              <table className="payslip-table" ref={tableRef}>
                <thead>
                  <tr>
                    {fields.map((field) => (
                      <th key={field.key} className={field.isNet ? 'col-net' : undefined}>
                        {field.label}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    {fields.map((field) => (
                      <td key={field.key} className={field.isNet ? 'col-net' : undefined}>
                        {field.value}
                      </td>
                    ))}
                  </tr>
                </tbody>
              </table>
            </div>

            <div className="warn-msg">⚠ 薪资保密，请勿讨论，违者按公司制度处理</div>
          </div>
        ) : (
          <div className="card">
            <div className="empty-hint">请选择月份查询工资</div>
          </div>
        )}
      </div>
    </>
  );
}
