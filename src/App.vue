<!--
  应用根只保留路由视图：欢迎页（V-WELCOME）与工作区（V-WORKSPACE）各自负责完整布局。
  历史 M0 脚手架（页面级 <h1>Knowl Pad</h1> + Markdown 渲染演示）已移除——
  它会在工作区上方多出一整块「Knowl Pad / Knowl Pad Markdown 渲染」，与真实界面重复。
-->
<template>
  <router-view />
</template>
