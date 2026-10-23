import { mount } from 'svelte'
import App from './App.svelte'
import './styles/base.css'
import './styles/entry.css'

const app = mount(App, { target: document.getElementById('app')! })

export default app
