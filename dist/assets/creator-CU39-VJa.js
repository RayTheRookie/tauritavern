import{i as l,t as Z}from"./core-B8jQGe47.js";const ee="modulepreload",te=function(e){return"/"+e},U={},ne=function(t,n,i){let s=Promise.resolve();if(n&&n.length>0){let o=function(c){return Promise.all(c.map(h=>Promise.resolve(h).then(b=>({status:"fulfilled",value:b}),b=>({status:"rejected",reason:b}))))};document.getElementsByTagName("link");const d=document.querySelector("meta[property=csp-nonce]"),g=(d==null?void 0:d.nonce)||(d==null?void 0:d.getAttribute("nonce"));s=o(n.map(c=>{if(c=te(c),c in U)return;U[c]=!0;const h=c.endsWith(".css"),b=h?'[rel="stylesheet"]':"";if(document.querySelector(`link[href="${c}"]${b}`))return;const f=document.createElement("link");if(f.rel=h?"stylesheet":ee,h||(f.as="script"),f.crossOrigin="",f.href=c,g&&f.setAttribute("nonce",g),document.head.appendChild(f),h)return new Promise((C,x)=>{f.addEventListener("load",C),f.addEventListener("error",()=>x(new Error(`Unable to preload CSS for ${c}`)))})}))}function a(o){const d=new Event("vite:preloadError",{cancelable:!0});if(d.payload=o,window.dispatchEvent(d),!d.defaultPrevented)throw o}return s.then(o=>{for(const d of o||[])d.status==="rejected"&&a(d.reason);return t().catch(a)})};var q;(function(e){e.WINDOW_RESIZED="tauri://resize",e.WINDOW_MOVED="tauri://move",e.WINDOW_CLOSE_REQUESTED="tauri://close-requested",e.WINDOW_DESTROYED="tauri://destroyed",e.WINDOW_FOCUS="tauri://focus",e.WINDOW_BLUR="tauri://blur",e.WINDOW_SCALE_FACTOR_CHANGED="tauri://scale-change",e.WINDOW_THEME_CHANGED="tauri://theme-changed",e.WINDOW_CREATED="tauri://window-created",e.WINDOW_SUSPENDED="tauri://suspended",e.WINDOW_RESUMED="tauri://resumed",e.WEBVIEW_CREATED="tauri://webview-created",e.DRAG_ENTER="tauri://drag-enter",e.DRAG_OVER="tauri://drag-over",e.DRAG_DROP="tauri://drag-drop",e.DRAG_LEAVE="tauri://drag-leave"})(q||(q={}));async function re(e,t){window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(e,t),await l("plugin:event|unlisten",{event:e,eventId:t})}async function ie(e,t,n){var i;const s=(i=void 0)!==null&&i!==void 0?i:{kind:"Any"};return l("plugin:event|listen",{event:e,target:s,handler:Z(t)}).then(a=>async()=>re(e,a))}const r={workbenchId:null,manifest:{name:"",author:"",version:"1.0.0",description:"",entry_file:"ui/index.html",cover_image:""},preset:{system_prompt:"",prompt_entries:[],model:null,temperature:null,max_tokens:null,context_window_size:null,provider:null,provider_url:null,chat_format:null,authors_note:null,authors_note_depth:null,user_name:"User",char_name:"Character"},worldInfo:[],pipeline:{context_strategy:{max_context_tokens:8e3,rag_fetch_count:3,rag_similarity_threshold:.75,history_fetch_limit:100},regex_mutators:[]},uiFiles:{"index.html":`<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>My Character</title>
  <link rel="stylesheet" href="style.css" />
  <script src="../tauri-tavern-sdk.js"><\/script>
  <script defer src="script.js"><\/script>
</head>
<body>
  <h1>Hello!</h1>
</body>
</html>
`,"script.js":`const SDK = window.TavernSDK;

// Your chat logic here
`,"style.css":`body {
  font-family: sans-serif;
  background: #1a1a2e;
  color: #e0e0e0;
  margin: 0;
  padding: 16px;
}
`},agentConfig:{provider:"openai",model:"gpt-4o",provider_url:null,temperature:.2,max_tokens:4096},agentMessages:[],editorSettings:{fontSize:13,tabSize:2,wordWrap:!1,autoPreview:!0},activeTab:"metadata",dirty:new Set,autoSaveTimer:null,providers:[]},J=document.getElementById("tab-bar"),N=document.getElementById("dirty-indicator"),se=document.getElementById("workbench-id-display"),ae=document.getElementById("btn-save-all"),oe=document.getElementById("btn-export"),de=document.getElementById("btn-close-creator"),D=document.getElementById("cr-toast"),F=document.getElementById("agent-bubble"),P=document.getElementById("agent-panel"),R=document.getElementById("agent-close"),W=document.getElementById("agent-settings"),I=document.getElementById("agent-messages"),v=document.getElementById("agent-input"),_=document.getElementById("agent-send"),y=document.getElementById("sidebar-context"),p=document.getElementById("creator-preview-iframe"),L=document.getElementById("preview-status"),$=document.getElementById("preview-refresh"),j=document.getElementById("preview-focus-code"),ce=document.getElementById("left-sidebar"),le=document.getElementById("preview-sidebar"),ue=document.getElementById("left-resizer"),pe=document.getElementById("preview-resizer"),me=new Set(["send_chat","execute_st_command","dry_run_prompt_pipeline","create_chat","list_chats","get_messages","delete_chat","set_chat_variable","get_chat_variable","list_chat_variables","delete_chat_variable","load_asset","get_preset","match_world_info"]),ge=new Set(["chat-chunk"]),fe="allow-scripts allow-forms allow-modals";let k="";const A=new Map;document.addEventListener("DOMContentLoaded",async()=>{const t=new URLSearchParams(window.location.search).get("workbench");if(!t){m("No workbench ID provided","error");return}r.workbenchId=t,se.textContent=t.slice(0,8)+"...";try{r.providers=await l("get_providers"),await we(),ve(),Ie(),await E("metadata"),B(0)}catch(n){m("Failed to load: "+n,"error")}ae.addEventListener("click",()=>G()),oe.addEventListener("click",()=>Re()),de.addEventListener("click",()=>window.close()),Ee(),window.addEventListener("creator-edit-regex-replacement",async n=>{var s;const i=(s=n.detail)==null?void 0:s.index;Number.isInteger(i)&&(await E("ui-code"),window.dispatchEvent(new CustomEvent("creator-select-regex-replacement",{detail:{index:i}})))}),window.addEventListener("beforeunload",()=>{r.autoSaveTimer&&clearTimeout(r.autoSaveTimer),Q(),G()})});function ve(){J.querySelectorAll(".cr-tab").forEach(e=>{e.addEventListener("click",()=>E(e.dataset.tab))})}async function E(e){r.activeTab&&r.activeTab!==e&&await S(),r.activeTab=e,J.querySelectorAll(".cr-tab").forEach(n=>{n.classList.toggle("active",n.dataset.tab===e)}),document.querySelectorAll(".cr-panel").forEach(n=>n.classList.add("hidden"));const t=document.getElementById("panel-"+e);switch(t&&t.classList.remove("hidden"),e){case"metadata":await u("metadata-tab.js",t,"renderMetadata");break;case"preset":await u("preset-tab.js",t,"renderPreset");break;case"world-info":await u("world-info-tab.js",t,"renderWorldInfo");break;case"pipeline":await u("pipeline-tab.js",t,"renderPipeline");break;case"ui-code":await u("ui-code-tab.js",t,"renderUICode");break;case"test-chat":await u("test-chat-tab.js",t,"renderTestChat");break}Y(),B(0)}const O={};async function u(e,t,n){O[e]||(O[e]=await ne(()=>import("./tabs/"+e),[])),O[e][n](t,r)}async function we(){const e=await l("get_workbench",{workbenchId:r.workbenchId});if(e.manifest&&(r.manifest=e.manifest),e.preset&&(r.preset=e.preset),e.world_info&&(r.worldInfo=e.world_info),e.pipeline&&(r.pipeline=e.pipeline),e.agent_config&&(r.agentConfig=e.agent_config),e.ui_files)for(const[t,n]of Object.entries(e.ui_files))r.uiFiles[t]=n}function he(e){e.manifest&&(r.manifest=e.manifest),e.preset&&(r.preset=e.preset),e.world_info&&(r.worldInfo=e.world_info),e.pipeline&&(r.pipeline=e.pipeline),e.agent_config&&(r.agentConfig=e.agent_config),e.ui_files&&(r.uiFiles={...r.uiFiles,...e.ui_files})}function be(e){r.dirty.add(e),N.classList.remove("hidden")}async function S(){r.autoSaveTimer&&(clearTimeout(r.autoSaveTimer),r.autoSaveTimer=null);const e=[];for(const t of r.dirty)switch(t){case"manifest":e.push(l("save_workbench_manifest",{workbenchId:r.workbenchId,manifest:r.manifest}));break;case"preset":e.push(l("save_workbench_preset",{workbenchId:r.workbenchId,preset:r.preset}));break;case"worldInfo":e.push(l("save_workbench_world_info",{workbenchId:r.workbenchId,entries:r.worldInfo}));break;case"pipeline":e.push(l("save_workbench_pipeline",{workbenchId:r.workbenchId,pipeline:r.pipeline}));break;case"uiFiles":for(const[n,i]of Object.entries(r.uiFiles))e.push(l("save_workbench_ui_file",{workbenchId:r.workbenchId,filename:n,content:i}));break}if(e.length>0)try{await Promise.all(e),r.dirty.clear(),N.classList.add("hidden")}catch(t){m("Save failed: "+t,"error")}}function ye(e){be(e),r.autoSaveTimer&&clearTimeout(r.autoSaveTimer),r.autoSaveTimer=setTimeout(()=>S(),2e3)}async function G(){r.dirty.add("manifest"),r.dirty.add("preset"),r.dirty.add("worldInfo"),r.dirty.add("pipeline"),r.dirty.add("uiFiles"),await S(),m("All saved","success")}async function _e(){const e=document.getElementById("panel-"+r.activeTab);switch(r.activeTab){case"metadata":await u("metadata-tab.js",e,"renderMetadata");break;case"preset":await u("preset-tab.js",e,"renderPreset");break;case"world-info":await u("world-info-tab.js",e,"renderWorldInfo");break;case"pipeline":await u("pipeline-tab.js",e,"renderPipeline");break;case"ui-code":await u("ui-code-tab.js",e,"renderUICode");break;case"test-chat":await u("test-chat-tab.js",e,"renderTestChat");break}}function Ee(){!F||!P||(F.addEventListener("click",()=>{P.classList.toggle("hidden"),v==null||v.focus()}),R==null||R.addEventListener("click",()=>P.classList.add("hidden")),W==null||W.addEventListener("click",Ce),_==null||_.addEventListener("click",H),v==null||v.addEventListener("keydown",e=>{e.key==="Enter"&&!e.shiftKey&&(e.preventDefault(),H())}))}async function H(){var n;const e=v.value.trim();if(!e||_.disabled)return;await S(),v.value="",K("user",e),_.disabled=!0,v.disabled=!0;const t=K("assistant","Working...");try{const i=await l("creator_agent_chat",{request:{workbench_id:r.workbenchId,message:e,history:r.agentMessages.slice(-8)}});t.textContent=i.reply||"Done.",r.agentMessages.push({role:"user",content:e}),r.agentMessages.push({role:"assistant",content:i.reply||"Done."}),i.workbench&&(he(i.workbench),r.dirty.clear(),N.classList.add("hidden"),await _e(),B(0)),(n=i.applied)!=null&&n.length&&m("Agent updated: "+i.applied.join(", "),"success")}catch(i){t.textContent="Error: "+i,t.classList.add("error")}finally{_.disabled=!1,v.disabled=!1,v.focus(),I.scrollTop=I.scrollHeight}}function Ie(){p&&p.setAttribute("sandbox",fe),window.addEventListener("message",Ae),$==null||$.addEventListener("click",()=>B(0)),j==null||j.addEventListener("click",()=>E("ui-code")),V(ue,ce,"leftSidebarWidth",{min:180,max:420,defaultWidth:240,side:"left"}),V(pe,le,"previewSidebarWidth",{min:260,max:720,defaultWidth:420,side:"right"})}function V(e,t,n,i){if(!e||!t)return;const s=Number(localStorage.getItem(n)),a=Number.isFinite(s)&&s>0?s:i.defaultWidth;t.style.width=`${a}px`,e.addEventListener("pointerdown",o=>{o.preventDefault(),e.setPointerCapture(o.pointerId);const d=o.clientX,g=t.getBoundingClientRect().width,c=b=>{const f=b.clientX-d,C=i.side==="right"?g-f:g+f,x=Math.max(i.min,Math.min(i.max,C));t.style.width=`${x}px`,localStorage.setItem(n,String(x))},h=b=>{e.releasePointerCapture(b.pointerId),window.removeEventListener("pointermove",c),window.removeEventListener("pointerup",h)};window.addEventListener("pointermove",c),window.addEventListener("pointerup",h)})}function Y(){var i,s;if(!y)return;const e=Object.keys(r.uiFiles),t=(r.pipeline.regex_mutators||[]).map((a,o)=>({item:a,index:o})).filter(({item:a})=>ke(a)),n={metadata:"Metadata",preset:"Prompt Preset","world-info":"World Info",pipeline:"Pipeline","ui-code":"UI Code","test-chat":"Test Chat"};y.innerHTML=`
    <div class="sidebar-section">
      <div class="sidebar-section-title">Active</div>
      <div class="sidebar-row strong">${w(n[r.activeTab]||r.activeTab)}</div>
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">UI Files</div>
      ${e.map(a=>`<button class="sidebar-row sidebar-file" data-file="${w(a)}">${w(a)}</button>`).join("")}
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">Regex</div>
      <button class="sidebar-row sidebar-regex-manage">Manage Regex</button>
      ${t.length?t.map(({item:a,index:o})=>`<button class="sidebar-row sidebar-regex" data-index="${o}">${w(a.id||"Regex #"+(o+1))}</button>`).join(""):'<button class="sidebar-row sidebar-regex-new">+ Display Regex</button>'}
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">Card</div>
      <div class="sidebar-row">${w(r.manifest.name||"Untitled")}</div>
      <div class="sidebar-muted">${w(r.manifest.author||"Anonymous")}</div>
    </div>
  `,y.querySelectorAll(".sidebar-file").forEach(a=>{a.addEventListener("click",async()=>{await E("ui-code"),window.dispatchEvent(new CustomEvent("creator-select-ui-file",{detail:{file:a.dataset.file}}))})}),y.querySelectorAll(".sidebar-regex").forEach(a=>{a.addEventListener("click",()=>{window.dispatchEvent(new CustomEvent("creator-edit-regex-replacement",{detail:{index:parseInt(a.dataset.index)}}))})}),(i=y.querySelector(".sidebar-regex-manage"))==null||i.addEventListener("click",()=>E("pipeline")),(s=y.querySelector(".sidebar-regex-new"))==null||s.addEventListener("click",async()=>{r.pipeline.regex_mutators.push({id:`display_regex_${r.pipeline.regex_mutators.length+1}`,enabled:!0,placement:"display",target:"display",depth_range:[],pattern:"<Gui>([\\s\\S]*?)</Gui>",replacement:"$1",flags:"gs",sample:"<Gui><div>Hello</div></Gui>",description:"Frontend display replacement",markdown_only:!0,prompt_only:!1,run_on_edit:!0}),ye("pipeline");const a=r.pipeline.regex_mutators.length-1;Y(),window.dispatchEvent(new CustomEvent("creator-edit-regex-replacement",{detail:{index:a}}))})}function ke(e){const t=String(e.placement||"").toLowerCase(),n=String(e.target||"").toLowerCase();return e.prompt_only||t==="prompt"?!1:e.markdown_only||t==="display"||t==="ui_display"?!0:["display","frontend","message_display","bot_output"].includes(n)}let z;function B(e=120){clearTimeout(z),z=setTimeout(Se,e)}function Se(){if(!(!p||!r.workbenchId))try{Q(),k=Le(),p.srcdoc=xe(),L&&(L.textContent="SDK sandbox · "+new Date().toLocaleTimeString())}catch(e){L&&(L.textContent="Preview error"),m("Preview failed: "+e,"error")}}function xe(){const e=`tavern://localhost/workbench/${r.workbenchId}/ui/`,t="tavern://localhost/sdk/tauri-tavern-sdk.js";let n=r.uiFiles["index.html"]||"<!doctype html><html><head></head><body></body></html>";const i=r.uiFiles["style.css"]||"",s=r.uiFiles["script.js"]||"";n=n.replace(/<link\b[^>]*href=["'](?:\.\/)?style\.css["'][^>]*>/gi,"").replace(/<script\b[^>]*src=["'](?:\.\/)?script\.js["'][^>]*>\s*<\/script>/gi,"").replace(/<script\b[^>]*src=["']\.\.\/tauri-tavern-sdk\.js["'][^>]*>\s*<\/script>/gi,"");const a=`<base href="${e}"><script>${Te()}<\/script><script src="${t}"><\/script><style data-live-style>${i}</style>`,o=`<script data-live-script>${s.replace(/<\/script/gi,"<\\/script")}<\/script>`;return/<head[^>]*>/i.test(n)?n=n.replace(/<head[^>]*>/i,d=>`${d}${a}`):n=/<html[^>]*>/i.test(n)?n.replace(/<html[^>]*>/i,d=>`${d}<head>${a}</head>`):`<head>${a}</head>${n}`,/<\/body>/i.test(n)?n=n.replace(/<\/body>/i,`${o}</body>`):n+=o,n}function Le(){var t;return`creator-preview-${(t=window.crypto)!=null&&t.getRandomValues?Array.from(window.crypto.getRandomValues(new Uint32Array(2)),n=>n.toString(16)).join(""):String(Date.now())}`}function Te(){const e=JSON.stringify(k),t=JSON.stringify(r.workbenchId);return`
(() => {
  const previewToken = ${e};
  const cartridgeId = ${t};
  const pending = new Map();
  const listeners = new Map();
  let nextId = 1;
  let nextListenerId = 1;

  window.addEventListener("message", event => {
    const message = event.data || {};
    if (message.source === "tauri-tavern-creator-event" && message.previewToken === previewToken) {
      const handler = listeners.get(message.listenerId);
      if (handler) handler(message.event);
      return;
    }
    if (message.source !== "tauri-tavern-creator" || message.previewToken !== previewToken) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.ok) {
      request.resolve(message.value);
    } else {
      request.reject(new Error(message.error || "Preview bridge request failed."));
    }
  });

  function request(type, payload) {
    const id = String(nextId++);
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      window.parent.postMessage({
        source: "tauri-tavern-preview",
        previewToken,
        id,
        type,
        ...payload
      }, "*");
    });
  }

  const bridge = Object.freeze({
    invoke(command, args) {
      return request("invoke", { command, args: args || {} });
    },
    listen(event, handler) {
      const listenerId = String(nextListenerId++);
      listeners.set(listenerId, typeof handler === "function" ? handler : () => {});
      return request("listen", { event, listenerId }).then(() => {
        return () => {
          listeners.delete(listenerId);
          request("unlisten", { listenerId }).catch(console.warn);
        };
      }).catch(error => {
        listeners.delete(listenerId);
        throw error;
      });
    },
    cartridgeId
  });

  Object.defineProperty(window, "__TAURI_TAVERN_BRIDGE__", {
    value: bridge,
    enumerable: false,
    configurable: false,
    writable: false
  });
  try { delete window.__TAURI__; } catch (_) {}
  try { delete window.__TAURI_INTERNALS__; } catch (_) {}
  try {
    Object.defineProperty(window, "__TAURI__", { value: undefined, configurable: false, writable: false });
  } catch (_) {}
  try {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: undefined, configurable: false, writable: false });
  } catch (_) {}
})();
`.replace(/<\/script/gi,"<\\/script")}async function Ae(e){if(!p||e.source!==p.contentWindow)return;const t=e.data||{};if(!(t.source!=="tauri-tavern-preview"||t.previewToken!==k))try{if(t.type==="invoke"){const n=String(t.command||"");if(!me.has(n))throw new Error(`Command '${n}' is not exposed to the Creator preview sandbox.`);const i=await l(n,Be(n,t.args));T(t.id,!0,i);return}if(t.type==="listen"){const n=String(t.event||"");if(!ge.has(n))throw new Error(`Event '${n}' is not exposed to the Creator preview sandbox.`);const i=String(t.listenerId||"");if(!i)throw new Error("Preview event listener id is required.");M(i);const s=await ie(n,a=>{var o;(o=p==null?void 0:p.contentWindow)==null||o.postMessage({source:"tauri-tavern-creator-event",previewToken:k,listenerId:i,event:a},"*")});A.set(i,s),T(t.id,!0,!0);return}if(t.type==="unlisten"){M(String(t.listenerId||"")),T(t.id,!0,!0);return}throw new Error("Unknown preview bridge request.")}catch(n){T(t.id,!1,null,String((n==null?void 0:n.message)||n))}}function M(e){const t=A.get(e);if(t){A.delete(e);try{t()}catch(n){console.warn("Failed to clear preview event listener",n)}}}function Q(){for(const e of Array.from(A.keys()))M(e)}function Be(e,t){const n={...t||{}};if(!r.workbenchId)return n;if(n.cartridgeId||(n.cartridgeId=r.workbenchId),n.cartridgeId!==r.workbenchId)throw new Error("Creator preview scope mismatch.");return n}function T(e,t,n,i=""){var s;(s=p==null?void 0:p.contentWindow)==null||s.postMessage({source:"tauri-tavern-creator",previewToken:k,id:e,ok:t,value:n,error:i},"*")}function K(e,t){const n=document.createElement("div");return n.className="agent-msg "+e,n.textContent=t,I.appendChild(n),I.scrollTop=I.scrollHeight,n}function Ce(){const e=r.providers||[],t=r.agentConfig||{},n=document.createElement("div");n.innerHTML=`
    <div class="cr-modal-overlay" id="agent-settings-modal">
      <div class="cr-modal agent-settings-modal">
        <div class="cr-modal-header">
          <h3>Creator Agent Settings</h3>
          <button class="btn btn-ghost btn-sm" id="agent-settings-close">&times;</button>
        </div>
        <div class="cr-modal-body">
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Provider</span>
              <select id="agent-provider">
                ${e.map(s=>`<option value="${w(s.id)}" ${t.provider===s.id?"selected":""}>${w(s.display_name)}</option>`).join("")}
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Model</span><input id="agent-model" type="text" value="${w(t.model||"")}" /></label>
            </div>
          </div>
          <div class="cr-form-group">
            <label><span>API Key</span><input id="agent-api-key" type="password" placeholder="Stored in keyring for this provider" /></label>
          </div>
          <div class="cr-form-group">
            <label><span>Provider URL</span><input id="agent-provider-url" type="text" value="${w(t.provider_url||"")}" placeholder="Optional custom endpoint" /></label>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Temperature</span><input id="agent-temp" type="number" min="0" max="2" step="0.1" value="${t.temperature??.2}" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Max Tokens</span><input id="agent-max-tokens" type="number" min="512" step="256" value="${t.max_tokens??4096}" /></label>
            </div>
          </div>
        </div>
        <div class="cr-modal-footer">
          <button class="btn btn-ghost btn-sm" id="agent-fetch-models">Fetch Models</button>
          <button class="btn btn-primary btn-sm" id="agent-save-settings">Save Settings</button>
        </div>
      </div>
    </div>
  `,document.body.appendChild(n.firstElementChild);const i=()=>{var s;return(s=document.getElementById("agent-settings-modal"))==null?void 0:s.remove()};document.getElementById("agent-settings-close").addEventListener("click",i),document.getElementById("agent-save-settings").addEventListener("click",async()=>{try{await De(),i(),m("Agent settings saved","success")}catch(s){m("Agent settings failed: "+s,"error")}}),document.getElementById("agent-fetch-models").addEventListener("click",Pe)}async function De(){const e=document.getElementById("agent-provider").value,t=document.getElementById("agent-model").value.trim(),n=document.getElementById("agent-provider-url").value.trim(),i=document.getElementById("agent-api-key").value.trim();if(!e||!t)throw new Error("Provider and model are required");i&&await l("set_api_key",{provider:e,key:i});const s={provider:e,model:t,provider_url:n||null,temperature:parseFloat(document.getElementById("agent-temp").value),max_tokens:parseInt(document.getElementById("agent-max-tokens").value)};await l("save_creator_agent_config",{workbenchId:r.workbenchId,config:s}),r.agentConfig=s}async function Pe(){try{const e=document.getElementById("agent-provider").value,t=document.getElementById("agent-provider-url").value.trim(),i=document.getElementById("agent-api-key").value.trim()||await l("get_raw_api_key",{provider:e});if(!i)throw new Error("Enter an API key first");const s=r.providers.find(c=>c.id===e),a=t||(s==null?void 0:s.default_url)||"",o=await l("fetch_models",{provider:e,apiKey:i,apiUrl:a}),d=document.getElementById("agent-model");d.setAttribute("list","agent-model-list");let g=document.getElementById("agent-model-list");g||(g=document.createElement("datalist"),g.id="agent-model-list",document.body.appendChild(g)),g.innerHTML=o.map(c=>`<option value="${w(c)}"></option>`).join(""),!d.value&&o[0]&&(d.value=o[0]),m(`Loaded ${o.length} models`,"success")}catch(e){m("Fetch models failed: "+e,"error")}}async function Re(){await S();try{const e=await l("export_workbench",{workbenchId:r.workbenchId});m("Exported to "+e,"success")}catch(e){m("Export failed: "+e,"error")}}let X;function m(e,t){D.textContent=e,D.className="cr-toast "+t,clearTimeout(X),X=setTimeout(()=>D.classList.add("hidden"),3e3)}function w(e){const t=document.createElement("div");return t.textContent=e,t.innerHTML}
