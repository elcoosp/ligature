<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "App" });

const rows = ref(["alpha", "beta"]);
</script>

<template>
  <main><Badge v-for="t in rows" :key="t" :count="t" /></main>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: string }>();
</script>
<template><b class="badge">{{ count }}</b></template>
