(function () {
  var style = getComputedStyle(document.documentElement);
  var accent = style.getPropertyValue('--accent').trim();
  var accent2 = style.getPropertyValue('--accent2').trim();
  var ink = style.getPropertyValue('--ink').trim();
  var muted = style.getPropertyValue('--muted').trim();
  var rule = style.getPropertyValue('--rule').trim();
  var bg2 = style.getPropertyValue('--bg2').trim();
  var danger = style.getPropertyValue('--danger').trim();
  var warn = style.getPropertyValue('--warn').trim();

  // ---------- Chart 1: severity distribution ----------
  var el1 = document.getElementById('chart-severity');
  if (el1) {
    var c1 = echarts.init(el1, null, { renderer: 'svg' });
    c1.setOption({
      animation: false,
      grid: { left: 130, right: 40, top: 36, bottom: 30 },
      legend: {
        top: 0, right: 10, itemWidth: 12, itemHeight: 12,
        textStyle: { color: muted, fontSize: 12 }
      },
      tooltip: {
        trigger: 'axis', axisPointer: { type: 'shadow' }, appendToBody: true,
        textStyle: { fontSize: 12 }
      },
      xAxis: {
        type: 'value', splitLine: { lineStyle: { color: rule } },
        axisLabel: { color: muted, fontSize: 11 }
      },
      yAxis: {
        type: 'category',
        data: ['测试与质量保障', 'Web 前端与文档', '控制面板与部署链路', '后端核心（app/excel/cache）'],
        axisLabel: { color: ink, fontSize: 12 },
        axisLine: { lineStyle: { color: rule } },
        axisTick: { show: false }
      },
      series: [
        {
          name: '高危', type: 'bar', stack: 'total', barWidth: 22,
          itemStyle: { color: danger },
          label: { show: true, position: 'inside', color: '#fff', fontSize: 11, fontWeight: 700 },
          data: [3, 0, 2, 5]
        },
        {
          name: '中危', type: 'bar', stack: 'total',
          itemStyle: { color: warn },
          label: { show: true, position: 'inside', color: '#fff', fontSize: 11, fontWeight: 700 },
          data: [5, 8, 19, 15]
        },
        {
          name: '低危', type: 'bar', stack: 'total',
          itemStyle: { color: muted },
          label: { show: true, position: 'inside', color: '#fff', fontSize: 11, fontWeight: 700 },
          data: [2, 8, 11, 9]
        }
      ]
    });
    window.addEventListener('resize', function () { c1.resize(); });
  }

  // ---------- Chart 2: risk matrix ----------
  var el2 = document.getElementById('chart-risk');
  if (el2) {
    var high = [
      { name: '密码=手机号明文', value: [5.0, 5.0], lp: 'left' },
      { name: '工号即密码旁路', value: [4.8, 4.55], lp: 'left' },
      { name: '暴力枚举无限制', value: [4.55, 5.0], lp: 'bottom' },
      { name: 'HTTP 明文传输', value: [4.2, 4.2], lp: 'left' },
      { name: 'xlsx 明文凭证', value: [4.8, 2.8], lp: 'right' },
      { name: 'GUI 崩溃服务全停', value: [3.8, 3.4], lp: 'bottom' },
      { name: '无运行日志（排障盲区）', value: [2.6, 3.0], lp: 'right' }
    ].map(function (d) {
      return { name: d.name, value: d.value, label: { position: d.lp } };
    });
    var mid = [
      { name: 'secret.key 泄露→会话伪造', value: [4.4, 2.2], lp: 'left' },
      { name: 'JS 注入 XSS', value: [3.4, 2.6], lp: 'bottom' },
      { name: '截图无水印外泄', value: [3.2, 4.0], lp: 'left' },
      { name: '日志含薪酬敏感数据', value: [2.8, 2.0], lp: 'left' },
      { name: '假测试假安全', value: [3.0, 2.35], lp: 'right' }
    ].map(function (d) {
      return { name: d.name, value: d.value, label: { position: d.lp } };
    });

    var c2 = echarts.init(el2, null, { renderer: 'svg' });
    c2.setOption({
      animation: false,
      grid: { left: 70, right: 30, top: 45, bottom: 55 },
      tooltip: {
        trigger: 'item', appendToBody: true,
        textStyle: { fontSize: 12 },
        formatter: function (p) {
          return p.name + '<br/>影响范围：' + p.value[0].toFixed(1) + ' / 5<br/>利用容易度：' + p.value[1].toFixed(1) + ' / 5';
        }
      },
      legend: {
        top: 0, right: 10, itemWidth: 12, itemHeight: 12,
        textStyle: { color: muted, fontSize: 12 },
        data: ['高危风险', '中危风险']
      },
      xAxis: {
        name: '影响范围 →', nameLocation: 'middle', nameGap: 30,
        nameTextStyle: { color: muted, fontSize: 12 },
        min: 1.5, max: 5.5, interval: 1,
        splitLine: { lineStyle: { color: rule } },
        axisLabel: { color: muted, fontSize: 11 }
      },
      yAxis: {
        name: '利用容易度 →', nameLocation: 'middle', nameGap: 40,
        nameTextStyle: { color: muted, fontSize: 12 },
        min: 1.5, max: 5.5, interval: 1,
        splitLine: { lineStyle: { color: rule } },
        axisLabel: { color: muted, fontSize: 11 }
      },
      series: [
        {
          name: '高危风险', type: 'scatter',
          data: high,
          symbolSize: 20,
          itemStyle: { color: danger, opacity: 0.9, borderColor: bg2, borderWidth: 2 },
          label: { show: true, formatter: '{b}', fontSize: 11, color: ink, distance: 8 },
          markLine: {
            silent: true, symbol: 'none',
            lineStyle: { color: danger, type: 'dashed', opacity: 0.35 },
            label: { show: false },
            data: [{ xAxis: 3.5 }, { yAxis: 3.5 }]
          },
          markArea: {
            silent: true,
            itemStyle: { color: danger, opacity: 0.06 },
            label: {
              show: true, position: 'insideTopRight', distance: 8,
              color: danger, fontSize: 12, fontWeight: 700,
              formatter: '高危红区'
            },
            data: [[{ xAxis: 3.5, yAxis: 3.5 }, { xAxis: 5.5, yAxis: 5.5 }]]
          }
        },
        {
          name: '中危风险', type: 'scatter',
          data: mid,
          symbolSize: 20,
          itemStyle: { color: warn, opacity: 0.9, borderColor: bg2, borderWidth: 2 },
          label: { show: true, formatter: '{b}', fontSize: 11, color: ink, distance: 8 }
        }
      ]
    });
    window.addEventListener('resize', function () { c2.resize(); });
  }
})();
