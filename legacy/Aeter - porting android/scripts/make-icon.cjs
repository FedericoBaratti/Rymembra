const sharp = require('sharp')
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256">
 <defs>
  <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
   <stop offset="0" stop-color="#1b1b27"/><stop offset="1" stop-color="#0a0a10"/>
  </linearGradient>
  <linearGradient id="ac" x1="0" y1="1" x2="0" y2="0">
   <stop offset="0" stop-color="#7c5cff"/><stop offset="1" stop-color="#b18cff"/>
  </linearGradient>
 </defs>
 <rect width="256" height="256" rx="56" fill="url(#bg)"/>
 <g fill="url(#ac)">
  <rect x="58"  y="118" width="16" height="40"  rx="8"/>
  <rect x="86"  y="92"  width="16" height="92"  rx="8"/>
  <rect x="114" y="64"  width="16" height="148" rx="8"/>
  <rect x="142" y="84"  width="16" height="108" rx="8"/>
  <rect x="170" y="104" width="16" height="68"  rx="8"/>
 </g>
</svg>`
sharp(Buffer.from(svg)).png().toFile('resources/icon-256.png').then(() => console.log('png ok'))
