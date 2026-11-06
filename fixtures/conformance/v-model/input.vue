<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Signup" });

const email = ref("");
</script>

<template>
  <form class="signup">
    <input v-model="email" />
    <p>email: {{ email }}</p>
  </form>
</template>
