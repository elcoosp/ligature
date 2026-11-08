<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: string }>();
</script>
<template>
  <b class="badge">{{ count }}</b>
</template>
<script setup lang="ts">
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header" />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);
const n = ref("two");
</script>
<template>
  <Panel title="Status">
    <template #header>
      <div>
        <Badge v-if="on" :count="n" />
      </div>
    </template>
  </Panel>
</template>
