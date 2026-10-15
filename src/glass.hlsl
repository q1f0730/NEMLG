// GlassDock — Studio Glass shader (D3D11, ps_5_0)
// Output is premultiplied alpha for a DirectComposition swap chain.

cbuffer C : register(b0)
{
    float4 rectPx;    // glass rect in window px: x, y, w, h
    float4 winScreen; // window origin in capture px (x, y), capture size (w, h)
    float4 p0;        // radius, refraction px, edge band px, blur lod
    float4 p1;        // aberration, saturation, brightness, rim alpha
    float4 tint;      // rgb, amount
    float4 p2;        // shadow alpha, shadow blur px, shadow y px, hover 0..1
    float4 light;     // light dir xy, sheen, press 0..1
    float4 p3;        // scale (px per dip), refraction sign, specular, adaptive (darken bright backdrops for white text)
};

Texture2D desk : register(t0);
SamplerState samp : register(s0);

struct VO { float4 pos : SV_Position; };

VO vs(uint id : SV_VertexID)
{
    VO o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

float sdRound(float2 p, float2 b, float r)
{
    float2 q = abs(p) - b + r;
    return length(max(q, 0)) + min(max(q.x, q.y), 0) - r;
}

float3 saturateColor(float3 c, float s)
{
    float l = dot(c, float3(0.2126, 0.7152, 0.0722));
    return lerp(l.xxx, c, s);
}

static const float2 K[8] = {
    float2( 0.99,  0.10), float2(-0.10,  0.99), float2(-0.99, -0.10), float2( 0.10, -0.99),
    float2( 0.62,  0.62), float2(-0.62,  0.62), float2(-0.62, -0.62), float2( 0.62, -0.62)
};

float3 blurAt(float2 px, float lod)
{
    float2 inv = 1.0 / winScreen.zw;
    float r = exp2(lod) * 0.85;
    float3 acc = desk.SampleLevel(samp, px * inv, lod).rgb * 2.0;
    [unroll] for (int i = 0; i < 8; i++)
        acc += desk.SampleLevel(samp, (px + K[i] * r * (i < 4 ? 1.0 : 1.6)) * inv, lod).rgb;
    return acc / 10.0;
}

float4 ps(VO i) : SV_Target
{
    float s = p3.x;
    float2 p = i.pos.xy;
    float2 c = rectPx.xy + rectPx.zw * 0.5;
    float press = light.w;
    float2 b = rectPx.zw * 0.5 * (1.0 - 0.015 * press);
    float r = p0.x;
    float2 lp = p - c;
    float d = sdRound(lp, b, r);

    float2 e = float2(0.5, 0);
    float2 n = float2(sdRound(lp + e.xy, b, r) - sdRound(lp - e.xy, b, r),
                      sdRound(lp + e.yx, b, r) - sdRound(lp - e.yx, b, r));
    n = n / max(length(n), 1e-4);

    float hover = p2.w;

    // soft drop shadow
    float sd = sdRound(lp - float2(0, p2.z * (1.0 + 0.4 * hover)), b, r);
    float sh = p2.x * (1.0 + 0.25 * hover) * (1.0 - smoothstep(-p2.y * 0.5, p2.y, sd));
    float a = saturate(0.5 - d);
    float4 shadow = float4(0, 0, 0, sh * (1.0 - a));
    if (a <= 0.0) return shadow;

    // lens: refraction concentrated in the edge band
    float depth = saturate(-d / p0.z);
    float bend = p0.y * pow(1.0 - depth, 2.4) * p3.y;
    float2 screen = winScreen.xy + p;
    float2 off = -n * bend;
    float ab = p1.x;
    float lod = p0.w;

    float3 col;
    col.r = blurAt(screen + off * (1.0 + ab), lod).r;
    col.g = blurAt(screen + off, lod).g;
    col.b = blurAt(screen + off * (1.0 - ab), lod).b;

    col = saturateColor(col, p1.y) * p1.z;
    float lum = dot(col, float3(0.2126, 0.7152, 0.0722));
    col *= 1.0 - p3.w * smoothstep(0.35, 0.95, lum) * 0.55;
    col = lerp(col, tint.rgb, tint.a);

    // body light: soft top-down gradient + hover sheen
    float ty = saturate((p.y - rectPx.y) / rectPx.w);
    col += light.z * (1.0 - ty) * (0.05 + 0.04 * hover);
    col *= 1.0 - 0.06 * (1.0 - depth) - 0.04 * press;

    // specular lobe near the lit edge
    float2 L = normalize(light.xy);
    float lit = saturate(dot(n, -L));
    col += p3.z * pow(lit, 3.0) * (1.0 - smoothstep(0.0, 0.35, depth));

    // crisp rim: 1 dip outer line + faint inner line
    float w = 1.1 * s;
    float rimOuter = smoothstep(-w, -w * 0.25, d) * (1.0 - smoothstep(-0.25, 0.75, d));
    float rimInner = smoothstep(-w * 2.6, -w * 1.8, d) * (1.0 - smoothstep(-w * 1.8, -w * 1.0, d));
    float facing = abs(dot(n, L));
    float rimI = p1.w * (0.30 + 0.70 * facing) * (1.0 + 0.45 * hover);
    col = lerp(col, float3(1, 1, 1), saturate(rimOuter * rimI));
    col = lerp(col, float3(1, 1, 1), saturate(rimInner * rimI * 0.22));

    col = saturate(col);
    return float4(col * a, a) + shadow * (1.0 - a);
}
