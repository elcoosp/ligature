<script setup lang="ts">
defineOptions({ name: "Panel" });

const { title } = defineProps<{ title: string }>();
</script>

<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header">
      <em>no header</em>
    </slot>
    <slot />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
</script>

<template>
  <div>
    <Panel title="Status">
      <template #header>
        <h1>all green</h1>
      </template>
      <p>body text</p>
    </Panel>
    <Panel title="Bare">
      <p>other body</p>
    </Panel>
  </div>
</template>
